//! Sender-side video queue measurement.
//!
//! str0m paces video behind a leaky bucket that tolerates up to two seconds of queued media
//! before it forces the queue to drain. That queue is invisible from the outside, yet every
//! millisecond spent in it is added to what the user sees on the iPad. This module measures it
//! exactly by watching the packets that really leave the PC: the RTP header of an SRTP packet is
//! sent in clear, and its marker bit flags the last packet of each video frame.
//!
//! The capture thread reads the result and holds back encoding while earlier frames are still
//! waiting to leave; it then encodes the newest desktop image as soon as the path is free (latest
//! wins) instead of stacking stale frames behind each other. Holding back a frame *before* it is
//! encoded never breaks the decoder's reference chain, unlike dropping an encoded frame.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A frame whose last packet has not left after this long is forgotten. str0m forces its queue
/// to drain within two seconds, so an older entry can only mean a marker packet that was never
/// observed; forgetting it keeps a measurement error from holding back video indefinitely.
const STALE: Duration = Duration::from_secs(3);

/// Upper bound on tracked frames (several seconds of video at 120 FPS).
const MAX_TRACKED: usize = 512;

/// Queue state shared between the network task (writer) and the capture thread (reader).
pub struct SendBacklog {
    epoch: Instant,
    queued_frames: AtomicU32,
    /// Microseconds since `epoch` (plus one) at which the oldest queued frame was written; zero
    /// when no frame is queued.
    oldest_us: AtomicU64,
}

impl SendBacklog {
    pub fn new(epoch: Instant) -> SendBacklog {
        SendBacklog {
            epoch,
            queued_frames: AtomicU32::new(0),
            oldest_us: AtomicU64::new(0),
        }
    }

    /// Encoded frames written to the peer connection whose last packet has not left yet.
    pub fn queued_frames(&self) -> u32 {
        self.queued_frames.load(Ordering::Acquire)
    }

    /// How long the oldest queued frame has been waiting.
    pub fn oldest_age(&self, now: Instant) -> Option<Duration> {
        match self.oldest_us.load(Ordering::Acquire) {
            0 => None,
            stamp => {
                let written = self.epoch + Duration::from_micros(stamp - 1);
                Some(now.saturating_duration_since(written))
            }
        }
    }

    fn publish(&self, queued: usize, oldest: Option<Instant>) {
        let stamp = oldest
            .map(|at| at.saturating_duration_since(self.epoch).as_micros() as u64 + 1)
            .unwrap_or(0);
        self.oldest_us.store(stamp, Ordering::Release);
        self.queued_frames
            .store(queued.min(u32::MAX as usize) as u32, Ordering::Release);
    }
}

/// Time spent in the send queue by the frames that completed during one statistics window.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct QueueDelay {
    pub average_ms: f32,
    pub max_ms: f32,
}

/// Follows every video frame from `write` until its last packet is handed to the network.
/// Owned by the network task.
pub struct FrameTracker {
    frames: VecDeque<(u32, Instant)>,
    highest_seq: Option<u16>,
    delay_sum: Duration,
    delay_max: Duration,
    delay_count: u32,
    shared: std::sync::Arc<SendBacklog>,
}

impl FrameTracker {
    pub fn new(shared: std::sync::Arc<SendBacklog>) -> FrameTracker {
        FrameTracker {
            frames: VecDeque::new(),
            highest_seq: None,
            delay_sum: Duration::ZERO,
            delay_max: Duration::ZERO,
            delay_count: 0,
            shared,
        }
    }

    /// A frame with this RTP timestamp was written to the peer connection.
    pub fn on_frame_written(&mut self, rtp_timestamp: u32, at: Instant) {
        self.frames.push_back((rtp_timestamp, at));
        while self.frames.len() > MAX_TRACKED {
            self.frames.pop_front();
        }
        self.prune(at);
        self.publish();
    }

    /// A datagram left the PC. Only the final packet of each video frame is of interest.
    pub fn on_transmit(&mut self, datagram: &[u8], video_pt: u8, at: Instant) {
        let Some(packet) = RtpSummary::parse(datagram) else {
            return;
        };
        if packet.payload_type != video_pt || !packet.marker {
            return;
        }
        // A retransmission reuses the original sequence number (when no RTX stream is
        // negotiated); it must not complete a newer frame.
        if let Some(highest) = self.highest_seq {
            if (packet.sequence.wrapping_sub(highest) as i16) <= 0 {
                return;
            }
        }
        self.highest_seq = Some(packet.sequence);
        // The pacer releases one stream in order: this frame and every earlier one have left.
        let mut changed = false;
        while let Some(&(timestamp, written)) = self.frames.front() {
            if (packet.timestamp.wrapping_sub(timestamp) as i32) < 0 {
                break;
            }
            self.frames.pop_front();
            changed = true;
            if timestamp == packet.timestamp {
                let delay = at.saturating_duration_since(written);
                self.delay_sum += delay;
                self.delay_max = self.delay_max.max(delay);
                self.delay_count += 1;
            }
        }
        changed |= self.prune(at);
        if changed {
            self.publish();
        }
    }

    /// Queue delay of the frames completed since the previous call.
    pub fn take_delay(&mut self) -> QueueDelay {
        let result = if self.delay_count == 0 {
            QueueDelay::default()
        } else {
            QueueDelay {
                average_ms: self.delay_sum.as_secs_f32() * 1000.0 / self.delay_count as f32,
                max_ms: self.delay_max.as_secs_f32() * 1000.0,
            }
        };
        self.delay_sum = Duration::ZERO;
        self.delay_max = Duration::ZERO;
        self.delay_count = 0;
        result
    }

    fn prune(&mut self, now: Instant) -> bool {
        let mut pruned = false;
        while self
            .frames
            .front()
            .is_some_and(|(_, written)| now.saturating_duration_since(*written) > STALE)
        {
            self.frames.pop_front();
            pruned = true;
        }
        if pruned {
            tracing::debug!("forgot video frames whose last packet was never observed");
        }
        pruned
    }

    fn publish(&self) {
        self.shared
            .publish(self.frames.len(), self.frames.front().map(|(_, at)| *at));
    }
}

/// Fields of an RTP header needed here. SRTP leaves the header unencrypted.
struct RtpSummary {
    marker: bool,
    payload_type: u8,
    sequence: u16,
    timestamp: u32,
}

impl RtpSummary {
    fn parse(datagram: &[u8]) -> Option<RtpSummary> {
        if datagram.len() < 12 || datagram[0] >> 6 != 2 {
            // Too short, or not RTP/RTCP at all (STUN, DTLS and SCTP start differently).
            return None;
        }
        if (192..=223).contains(&datagram[1]) {
            // RTCP (RFC 5761 demultiplexing).
            return None;
        }
        Some(RtpSummary {
            marker: datagram[1] & 0x80 != 0,
            payload_type: datagram[1] & 0x7f,
            sequence: u16::from_be_bytes([datagram[2], datagram[3]]),
            timestamp: u32::from_be_bytes([datagram[4], datagram[5], datagram[6], datagram[7]]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const PT: u8 = 102;

    fn rtp(marker: bool, pt: u8, seq: u16, ts: u32) -> Vec<u8> {
        let mut packet = vec![0x80, (if marker { 0x80 } else { 0 }) | pt];
        packet.extend_from_slice(&seq.to_be_bytes());
        packet.extend_from_slice(&ts.to_be_bytes());
        packet.extend_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        packet.extend_from_slice(&[0u8; 20]);
        packet
    }

    fn tracker() -> (FrameTracker, Arc<SendBacklog>, Instant) {
        let epoch = Instant::now();
        let shared = Arc::new(SendBacklog::new(epoch));
        (FrameTracker::new(shared.clone()), shared, epoch)
    }

    #[test]
    fn a_frame_leaves_the_queue_with_its_marker_packet() {
        let (mut tracker, shared, t0) = tracker();
        tracker.on_frame_written(9000, t0);
        assert_eq!(shared.queued_frames(), 1);
        assert_eq!(
            shared.oldest_age(t0 + Duration::from_millis(4)),
            Some(Duration::from_millis(4))
        );

        // The first packets of the frame do not complete it.
        tracker.on_transmit(&rtp(false, PT, 1, 9000), PT, t0 + Duration::from_millis(1));
        assert_eq!(shared.queued_frames(), 1);

        tracker.on_transmit(&rtp(true, PT, 2, 9000), PT, t0 + Duration::from_millis(6));
        assert_eq!(shared.queued_frames(), 0);
        assert_eq!(shared.oldest_age(t0 + Duration::from_millis(7)), None);
        let delay = tracker.take_delay();
        assert!((delay.average_ms - 6.0).abs() < 0.01);
        assert!((delay.max_ms - 6.0).abs() < 0.01);
        assert_eq!(tracker.take_delay(), QueueDelay::default());
    }

    #[test]
    fn queued_frames_are_counted_in_order() {
        let (mut tracker, shared, t0) = tracker();
        tracker.on_frame_written(1000, t0);
        tracker.on_frame_written(2500, t0 + Duration::from_millis(16));
        tracker.on_frame_written(4000, t0 + Duration::from_millis(33));
        assert_eq!(shared.queued_frames(), 3);

        tracker.on_transmit(&rtp(true, PT, 10, 1000), PT, t0 + Duration::from_millis(40));
        assert_eq!(shared.queued_frames(), 2);
        // The oldest frame is now the second one.
        assert_eq!(
            shared.oldest_age(t0 + Duration::from_millis(40)),
            Some(Duration::from_millis(24))
        );
    }

    #[test]
    fn a_later_marker_completes_frames_whose_marker_was_missed() {
        let (mut tracker, shared, t0) = tracker();
        tracker.on_frame_written(1000, t0);
        tracker.on_frame_written(2500, t0);
        tracker.on_transmit(&rtp(true, PT, 7, 2500), PT, t0 + Duration::from_millis(5));
        assert_eq!(shared.queued_frames(), 0);
    }

    #[test]
    fn timestamps_wrap_around() {
        let (mut tracker, shared, t0) = tracker();
        tracker.on_frame_written(u32::MAX - 100, t0);
        tracker.on_frame_written(200, t0);
        tracker.on_transmit(&rtp(true, PT, 1, u32::MAX - 100), PT, t0);
        assert_eq!(shared.queued_frames(), 1);
        tracker.on_transmit(&rtp(true, PT, 2, 200), PT, t0);
        assert_eq!(shared.queued_frames(), 0);
    }

    #[test]
    fn other_traffic_and_retransmissions_are_ignored() {
        let (mut tracker, shared, t0) = tracker();
        tracker.on_frame_written(5000, t0);
        tracker.on_frame_written(6500, t0);
        tracker.on_transmit(&rtp(true, PT, 100, 5000), PT, t0);
        assert_eq!(shared.queued_frames(), 1);

        // RTX (another payload type), RTCP, DTLS/SCTP and STUN never complete a frame.
        tracker.on_transmit(&rtp(true, PT + 1, 101, 6500), PT, t0);
        let mut rtcp = rtp(false, 0, 0, 0);
        rtcp[1] = 200;
        tracker.on_transmit(&rtcp, PT, t0);
        tracker.on_transmit(&[23, 254, 253, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], PT, t0);
        tracker.on_transmit(&[0, 1, 0, 0, 0x21, 0x12, 0xa4, 0x42, 0, 0, 0, 0], PT, t0);
        // A retransmission of an older packet on the same payload type is not new progress.
        tracker.on_transmit(&rtp(true, PT, 100, 6500), PT, t0);
        assert_eq!(shared.queued_frames(), 1);

        tracker.on_transmit(&rtp(true, PT, 102, 6500), PT, t0);
        assert_eq!(shared.queued_frames(), 0);
    }

    #[test]
    fn sequence_numbers_wrap_around() {
        let (mut tracker, shared, t0) = tracker();
        tracker.on_frame_written(1, t0);
        tracker.on_frame_written(2, t0);
        tracker.on_transmit(&rtp(true, PT, u16::MAX, 1), PT, t0);
        tracker.on_transmit(&rtp(true, PT, 0, 2), PT, t0);
        assert_eq!(shared.queued_frames(), 0);
    }

    #[test]
    fn frames_never_seen_leaving_are_eventually_forgotten() {
        let (mut tracker, shared, t0) = tracker();
        tracker.on_frame_written(1000, t0);
        tracker.on_frame_written(2000, t0 + STALE + Duration::from_millis(1));
        assert_eq!(shared.queued_frames(), 1);
    }
}
