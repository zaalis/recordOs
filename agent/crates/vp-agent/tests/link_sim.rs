//! Simulated network link between the agent's real WebRTC sender configuration (`negotiate`,
//! str0m pacer and bandwidth estimator, `FrameTracker`) and a str0m receiver, in virtual time.
//!
//! It exists to compare frame-scheduling policies under Wi-Fi-like conditions — bottleneck rate,
//! jitter, and short medium stalls — which cannot be reproduced on a loopback connection.
//! Run with `cargo test -p vp-agent --test link_sim -- --nocapture` to see the comparison table.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use str0m::bwe::BweKind;
use str0m::change::{SdpAnswer, SdpOffer};
use str0m::format::Codec;
use str0m::media::{Direction, MediaKind, MediaTime, Mid, Pt};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, Rtc};
use vp_agent::config::CodecId;
use vp_agent::media::backlog::{FrameGate, FrameTracker, SendBacklog};
use vp_agent::media::rtc::negotiate;

// --- deterministic randomness ---------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn exponential(&mut self, mean: f64) -> f64 {
        -mean * (1.0 - self.unit()).ln()
    }
}

// --- the link ------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct LinkConfig {
    rate_bps: f64,
    propagation_ms: f64,
    jitter_ms: f64,
    /// Mean time between medium stalls (Wi-Fi contention, aggregation, roaming scans).
    stall_every_ms: f64,
    stall_ms: f64,
    buffer_ms: f64,
}

struct Datagram {
    to_receiver: bool,
    source: SocketAddr,
    destination: SocketAddr,
    data: Vec<u8>,
}

struct Link {
    config: LinkConfig,
    rng: Rng,
    free_at: Instant,
    next_stall: Instant,
    last_arrival: Instant,
    order: BinaryHeap<Reverse<(Instant, u64)>>,
    stored: HashMap<u64, Datagram>,
    counter: u64,
}

impl Link {
    fn new(config: LinkConfig, seed: u64, start: Instant) -> Link {
        let mut rng = Rng(seed);
        let next_stall = if config.stall_every_ms > 0.0 {
            start + Duration::from_secs_f64(rng.exponential(config.stall_every_ms) / 1000.0)
        } else {
            start + Duration::from_secs(1_000_000)
        };
        Link {
            config,
            rng,
            free_at: start,
            next_stall,
            last_arrival: start,
            order: BinaryHeap::new(),
            stored: HashMap::new(),
            counter: 0,
        }
    }

    fn send(&mut self, now: Instant, datagram: Datagram) {
        let mut start = self.free_at.max(now);
        if start.duration_since(now).as_secs_f64() * 1000.0 > self.config.buffer_ms {
            return; // tail drop
        }
        while start >= self.next_stall {
            start = self.next_stall + Duration::from_secs_f64(self.config.stall_ms / 1000.0);
            self.next_stall = start
                + Duration::from_secs_f64(
                    self.rng.exponential(self.config.stall_every_ms) / 1000.0,
                );
        }
        let bits = (datagram.data.len() + 28) as f64 * 8.0;
        let done = start + Duration::from_secs_f64(bits / self.config.rate_bps);
        self.free_at = done;
        let jitter = self.rng.unit() * self.config.jitter_ms;
        let arrival = (done
            + Duration::from_secs_f64((self.config.propagation_ms + jitter) / 1000.0))
        .max(self.last_arrival);
        self.last_arrival = arrival;
        self.counter += 1;
        self.order.push(Reverse((arrival, self.counter)));
        self.stored.insert(self.counter, datagram);
    }

    fn next_arrival(&self) -> Option<Instant> {
        self.order.peek().map(|Reverse((at, _))| *at)
    }

    fn take_due(&mut self, now: Instant) -> Vec<Datagram> {
        let mut due = vec![];
        while let Some(Reverse((at, id))) = self.order.peek().copied() {
            if at > now {
                break;
            }
            self.order.pop();
            due.push(self.stored.remove(&id).unwrap());
        }
        due
    }
}

// --- frame scheduling policies -------------------------------------------------------------

/// Decides at every 1 ms tick whether a due frame is encoded now.
trait Policy {
    fn allow(
        &mut self,
        now: Instant,
        queued: u32,
        oldest: Option<Duration>,
        interval: Duration,
    ) -> bool;
}

/// Encode every frame as soon as it is due (the behaviour before frame holding existed).
struct Immediate;
impl Policy for Immediate {
    fn allow(&mut self, _: Instant, _: u32, _: Option<Duration>, _: Duration) -> bool {
        true
    }
}

/// The holding rule that shipped in 37422fb: hold while one frame has been queued for a whole
/// frame interval or two are queued, for at most 250 ms.
#[derive(Default)]
struct ShippedHold {
    since: Option<Instant>,
}
impl Policy for ShippedHold {
    fn allow(
        &mut self,
        now: Instant,
        queued: u32,
        oldest: Option<Duration>,
        interval: Duration,
    ) -> bool {
        let busy = queued >= 2 || (queued >= 1 && oldest.is_some_and(|age| age >= interval));
        if busy {
            let started = *self.since.get_or_insert(now);
            if now.duration_since(started) < Duration::from_millis(250) {
                return false;
            }
        }
        self.since = None;
        true
    }
}

/// The production gate.
#[derive(Default)]
struct Production {
    gate: FrameGate,
}
impl Policy for Production {
    fn allow(
        &mut self,
        now: Instant,
        queued: u32,
        oldest: Option<Duration>,
        _interval: Duration,
    ) -> bool {
        self.gate.allow(now, queued, oldest, false)
    }
}

// --- one simulated session -----------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct Outcome {
    encoded_fps: f64,
    delivered_fps: f64,
    mean_latency_ms: f64,
    p95_latency_ms: f64,
    mean_target_kbps: f64,
    mean_queue_ms: f64,
    delivered_kbps: f64,
}

struct Peer {
    rtc: Rtc,
    address: SocketAddr,
    wake: Instant,
}

fn drive(
    peer: &mut Peer,
    now: Instant,
    outgoing: &mut Vec<Datagram>,
    events: &mut Vec<Event>,
    to_receiver: bool,
) {
    loop {
        match peer.rtc.poll_output().expect("poll") {
            Output::Timeout(at) => {
                peer.wake = at.max(now + Duration::from_micros(200));
                break;
            }
            Output::Transmit(t) => outgoing.push(Datagram {
                to_receiver,
                source: t.source,
                destination: t.destination,
                data: t.contents.to_vec(),
            }),
            Output::Event(e) => events.push(e),
        }
    }
}

fn deliver(peer: &mut Peer, now: Instant, datagram: &Datagram) {
    let receive = Receive {
        proto: Protocol::Udp,
        source: datagram.source,
        destination: datagram.destination,
        contents: datagram.data.as_slice().try_into().expect("datagram"),
    };
    peer.rtc
        .handle_input(Input::Receive(now, receive))
        .expect("receive");
}

fn run_session(
    forward: LinkConfig,
    seconds: u64,
    warmup: u64,
    fps: u32,
    policy: &mut dyn Policy,
) -> Outcome {
    let base = Instant::now();
    let provider = Arc::new(str0m::crypto::from_feature_flags());
    let mut receiver = Peer {
        rtc: Rtc::builder().set_crypto_provider(provider).build(base),
        address: (Ipv4Addr::new(10, 0, 0, 2), 5000).into(),
        wake: base,
    };
    receiver
        .rtc
        .add_local_candidate(Candidate::host(receiver.address, "udp").unwrap());
    let mut change = receiver.rtc.sdp_api();
    change.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
    let (offer, pending) = change.apply().unwrap();

    // The sender is the agent's own negotiation: same codecs, extensions, BWE and pacer setup.
    let initial_kbps = 12_000;
    let negotiated = negotiate(
        &offer.to_sdp_string(),
        CodecId::H264,
        &[Ipv4Addr::LOCALHOST],
        initial_kbps,
        None,
    )
    .expect("negotiate");
    let sender_address = negotiated.sockets[0].local_addr().unwrap();
    receiver
        .rtc
        .sdp_api()
        .accept_answer(
            pending,
            SdpAnswer::from_sdp_string(&negotiated.answer).unwrap(),
        )
        .unwrap();
    let mut sender = Peer {
        rtc: negotiated.rtc,
        address: sender_address,
        wake: base,
    };
    let _ = SdpOffer::from_sdp_string(&offer.to_sdp_string()).unwrap();

    let mut to_receiver = Link::new(forward, 11, base);
    let mut to_sender = Link::new(
        LinkConfig {
            rate_bps: 50e6,
            stall_every_ms: 0.0,
            ..forward
        },
        29,
        base,
    );

    let epoch = base;
    let backlog = Arc::new(SendBacklog::new(epoch));
    let mut tracker = FrameTracker::new(backlog.clone());
    let mut rng = Rng(7);

    let interval = Duration::from_secs_f64(1.0 / fps as f64);
    let mut now = base;
    let end = base + Duration::from_secs(seconds);
    let measure_from = base + Duration::from_secs(warmup);

    let mut connected = false;
    let mut media: Option<(Mid, Pt)> = None;
    let mut video_pt = 0u8;
    let mut target_kbps = 4_000.0f64;
    let mut last_encode = base;
    let mut first = true;
    let mut next_app = base;

    let (mut encoded, mut delivered, mut delivered_bytes) = (0u64, 0u64, 0u64);
    let mut latencies: Vec<f64> = vec![];
    let (mut target_sum, mut target_n) = (0.0, 0u64);
    let (mut queue_sum, mut queue_n) = (0.0f64, 0u64);
    let mut last_queue_sample = base;

    while now < end {
        let mut next = sender.wake.min(receiver.wake).min(next_app).min(end);
        for link in [&to_receiver, &to_sender] {
            if let Some(at) = link.next_arrival() {
                next = next.min(at);
            }
        }
        now = next.max(now);

        let mut out = vec![];
        let mut sender_events = vec![];
        let mut receiver_events = vec![];

        for datagram in to_receiver.take_due(now) {
            deliver(&mut receiver, now, &datagram);
            receiver.wake = now;
        }
        for datagram in to_sender.take_due(now) {
            deliver(&mut sender, now, &datagram);
            sender.wake = now;
        }
        if now >= sender.wake {
            sender.rtc.handle_input(Input::Timeout(now)).unwrap();
            drive(&mut sender, now, &mut out, &mut sender_events, true);
        }
        if now >= receiver.wake {
            receiver.rtc.handle_input(Input::Timeout(now)).unwrap();
            drive(&mut receiver, now, &mut out, &mut receiver_events, false);
        }

        // Application: one tick per millisecond, like the capture loop's polling.
        if now >= next_app {
            next_app += Duration::from_millis(1);
            if connected {
                if let Some((mid, pt)) = media {
                    let since = now.duration_since(last_encode);
                    let due = first || since + Duration::from_millis(1) >= interval;
                    let queued = backlog.queued_frames();
                    if due
                        && (first || policy.allow(now, queued, backlog.oldest_age(now), interval))
                    {
                        let frame_ms = 1000.0 / fps as f64;
                        let vbv_bits = target_kbps * (2.0 * frame_ms).min(40.0).max(frame_ms);
                        let budget = target_kbps * 1000.0 / fps as f64;
                        let complexity = if rng.unit() < 0.04 {
                            3.0
                        } else {
                            0.55 + 0.4 * rng.unit()
                        };
                        let mut bits = (budget * complexity).min(vbv_bits.max(50_000.0));
                        if first {
                            bits = 200_000.0; // first keyframe
                        }
                        let bytes = ((bits / 8.0) as usize).max(300);
                        let mut data = vec![0xABu8; bytes];
                        data[..4].copy_from_slice(&[0, 0, 0, 1]);
                        data[4] = if first { 0x65 } else { 0x41 };
                        let rtp_time = now.duration_since(epoch).as_micros() as u64 * 9 / 100;
                        let wrote = sender
                            .rtc
                            .writer(mid)
                            .unwrap()
                            .playout_delay(MediaTime::ZERO, MediaTime::ZERO)
                            .write(pt, now, MediaTime::from_90khz(rtp_time), data)
                            .is_ok();
                        if wrote {
                            tracker.on_frame_written(rtp_time as u32, now);
                            last_encode = now;
                            first = false;
                            if now >= measure_from {
                                encoded += 1;
                            }
                        }
                        sender.wake = now;
                    }
                }
            }
        }

        // Anything the sender wrote must be polled promptly.
        if sender.wake <= now {
            drive(&mut sender, now, &mut out, &mut sender_events, true);
        }

        for datagram in out {
            if datagram.to_receiver {
                tracker.on_transmit(&datagram.data, video_pt, now);
                to_receiver.send(now, datagram);
            } else {
                to_sender.send(now, datagram);
            }
        }

        for event in sender_events {
            match event {
                Event::Connected => connected = true,
                Event::MediaAdded(added) if added.kind == MediaKind::Video => {
                    let pt = sender
                        .rtc
                        .writer(added.mid)
                        .unwrap()
                        .payload_params()
                        .find(|p| p.spec().codec == Codec::H264)
                        .map(|p| p.pt())
                        .expect("h264 payload type");
                    video_pt = *pt;
                    media = Some((added.mid, pt));
                }
                Event::EgressBitrateEstimate(BweKind::Twcc(bitrate)) => {
                    let estimate = bitrate.as_u64() as f64 / 1000.0;
                    target_kbps = (estimate * 0.85).clamp(1_000.0, 40_000.0);
                }
                _ => {}
            }
        }
        for event in receiver_events {
            if let Event::MediaData(data) = event {
                if now >= measure_from {
                    delivered += 1;
                    delivered_bytes += data.data.len() as u64;
                    let captured = data.time.as_seconds();
                    let latency = now.duration_since(epoch).as_secs_f64() - captured;
                    latencies.push(latency * 1000.0);
                }
            }
        }
        if now >= measure_from {
            target_sum += target_kbps;
            target_n += 1;
            if now.duration_since(last_queue_sample) >= Duration::from_secs(1) {
                last_queue_sample = now;
                let delay = tracker.take_delay();
                queue_sum += delay.average_ms as f64;
                queue_n += 1;
            }
        }
    }

    let measured = (seconds - warmup) as f64;
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = if latencies.is_empty() {
        0.0
    } else {
        latencies.iter().sum::<f64>() / latencies.len() as f64
    };
    let p95 = latencies
        .get((latencies.len() as f64 * 0.95) as usize)
        .copied()
        .unwrap_or(0.0);
    Outcome {
        encoded_fps: encoded as f64 / measured,
        delivered_fps: delivered as f64 / measured,
        mean_latency_ms: mean,
        p95_latency_ms: p95,
        mean_target_kbps: target_sum / target_n.max(1) as f64,
        mean_queue_ms: queue_sum / queue_n.max(1) as f64,
        delivered_kbps: delivered_bytes as f64 * 8.0 / 1000.0 / measured,
    }
}

// --- scenarios -----------------------------------------------------------------------------

const SCENARIOS: &[(&str, LinkConfig)] = &[
    (
        "ethernet 1 Gb/s",
        LinkConfig {
            rate_bps: 1e9,
            propagation_ms: 0.5,
            jitter_ms: 0.2,
            stall_every_ms: 0.0,
            stall_ms: 0.0,
            buffer_ms: 300.0,
        },
    ),
    (
        "wifi good 40 Mb/s",
        LinkConfig {
            rate_bps: 40e6,
            propagation_ms: 2.0,
            jitter_ms: 3.0,
            stall_every_ms: 500.0,
            stall_ms: 15.0,
            buffer_ms: 300.0,
        },
    ),
    (
        "wifi busy 10 Mb/s",
        LinkConfig {
            rate_bps: 10e6,
            propagation_ms: 3.0,
            jitter_ms: 8.0,
            stall_every_ms: 250.0,
            stall_ms: 30.0,
            buffer_ms: 300.0,
        },
    ),
    (
        "wifi weak 4 Mb/s",
        LinkConfig {
            rate_bps: 4e6,
            propagation_ms: 4.0,
            jitter_ms: 12.0,
            stall_every_ms: 200.0,
            stall_ms: 40.0,
            buffer_ms: 300.0,
        },
    ),
];

fn table(fps: u32, seconds: u64, warmup: u64) -> Vec<(String, String, Outcome)> {
    let mut rows = vec![];
    for (scenario, link) in SCENARIOS {
        let policies: Vec<(&str, Box<dyn Policy>)> = vec![
            ("immediate", Box::new(Immediate)),
            ("shipped hold", Box::<ShippedHold>::default()),
            ("production", Box::<Production>::default()),
        ];
        for (name, mut policy) in policies {
            let outcome = run_session(*link, seconds, warmup, fps, policy.as_mut());
            println!(
                "{scenario:<20} {name:<13} enc {:5.1} fps  shown {:5.1} fps  lat {:6.1}/{:6.1} ms (mean/p95)  target {:6.0} kb/s  queue {:5.1} ms  goodput {:6.0} kb/s",
                outcome.encoded_fps,
                outcome.delivered_fps,
                outcome.mean_latency_ms,
                outcome.p95_latency_ms,
                outcome.mean_target_kbps,
                outcome.mean_queue_ms,
                outcome.delivered_kbps,
            );
            rows.push((scenario.to_string(), name.to_string(), outcome));
        }
    }
    rows
}

#[test]
fn frame_scheduling_policies_compared() {
    let rows = table(60, 40, 10);
    let find = |scenario: &str, policy: &str| {
        rows.iter()
            .find(|(s, p, _)| s.starts_with(scenario) && p == policy)
            .map(|(_, _, o)| o.clone())
            .unwrap()
    };
    for (scenario, _) in SCENARIOS {
        let baseline = find(scenario, "immediate");
        let production = find(scenario, "production");
        // The gate must never cost frame rate: it may only cut queueing delay.
        assert!(
            production.delivered_fps >= baseline.delivered_fps * 0.9,
            "{scenario}: production {:.1} fps vs immediate {:.1} fps",
            production.delivered_fps,
            baseline.delivered_fps
        );
        assert!(
            production.mean_latency_ms <= baseline.mean_latency_ms * 1.1 + 5.0,
            "{scenario}: production latency {:.1} ms vs immediate {:.1} ms",
            production.mean_latency_ms,
            baseline.mean_latency_ms
        );
    }
}
