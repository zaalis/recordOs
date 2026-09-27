/*
 * Vision-Pro macOS native bridge — see vp_macos.h for the contract.
 * Compiled with ARC (-fobjc-arc) by build.rs for macOS 13 and newer.
 */
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#import <IOKit/pwr_mgt/IOPMLib.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <Security/Security.h>
#import <VideoToolbox/VideoToolbox.h>
#include <math.h>
#include <string.h>
#include <sys/sysctl.h>

#include "vp_macos.h"

static void vp_copy_string(NSString *value, char *out, size_t len) {
    if (!out || len == 0) return;
    const char *utf8 = value.UTF8String;
    strlcpy(out, utf8 ? utf8 : "", len);
}

static void vp_error(char *out, size_t len, NSString *format, ...) NS_FORMAT_FUNCTION(3, 4);
static void vp_error(char *out, size_t len, NSString *format, ...) {
    va_list args;
    va_start(args, format);
    NSString *message = [[NSString alloc] initWithFormat:format arguments:args];
    va_end(args);
    vp_copy_string(message, out, len);
}

static BOOL vp_wait(dispatch_semaphore_t semaphore, double seconds) {
    return dispatch_semaphore_wait(semaphore, dispatch_time(DISPATCH_TIME_NOW, (int64_t)(seconds * NSEC_PER_SEC))) == 0;
}

/* ============================================================================================ */
/* Displays                                                                                     */
/* ============================================================================================ */

int32_t vp_displays(vp_display_t *out, int32_t capacity) {
    if (!out || capacity <= 0) return 0;
    @autoreleasepool {
        CGDirectDisplayID ids[32];
        uint32_t count = 0;
        if (CGGetActiveDisplayList(32, ids, &count) != kCGErrorSuccess) return 0;

        NSMutableDictionary<NSNumber *, NSString *> *names = [NSMutableDictionary dictionary];
        @try {
            for (NSScreen *screen in [NSScreen screens]) {
                NSNumber *number = screen.deviceDescription[@"NSScreenNumber"];
                NSString *name = screen.localizedName;
                if (number && name.length) names[number] = name;
            }
        } @catch (NSException *exception) {
            /* Names are cosmetic; CoreGraphics still provides geometry. */
        }

        CGDirectDisplayID main = CGMainDisplayID();
        int32_t written = 0;
        int32_t external = 0;
        for (uint32_t i = 0; i < count && written < capacity; i++) {
            CGDirectDisplayID display = ids[i];
            /* A display that mirrors another one shows the same pixels: expose the source only. */
            if (CGDisplayMirrorsDisplay(display) != kCGNullDirectDisplay) continue;

            vp_display_t *entry = &out[written];
            memset(entry, 0, sizeof *entry);
            CGRect bounds = CGDisplayBounds(display);
            entry->id = display;
            entry->x = bounds.origin.x;
            entry->y = bounds.origin.y;
            entry->width = bounds.size.width;
            entry->height = bounds.size.height;
            entry->primary = display == main;
            entry->builtin = CGDisplayIsBuiltin(display);

            double refresh = 0;
            CGDisplayModeRef mode = CGDisplayCopyDisplayMode(display);
            if (mode) {
                entry->pixel_width = (uint32_t)CGDisplayModeGetPixelWidth(mode);
                entry->pixel_height = (uint32_t)CGDisplayModeGetPixelHeight(mode);
                refresh = CGDisplayModeGetRefreshRate(mode);
                CGDisplayModeRelease(mode);
            }
            if (entry->pixel_width == 0 || entry->pixel_height == 0) {
                entry->pixel_width = (uint32_t)CGDisplayPixelsWide(display);
                entry->pixel_height = (uint32_t)CGDisplayPixelsHigh(display);
            }
            /* Some built-in and adaptive-sync panels report 0 Hz: assume the 60 Hz baseline. */
            entry->refresh_hz = refresh >= 1 ? (uint32_t)lround(refresh) : 60;

            NSString *name = names[@(display)];
            if (!name.length) {
                name = entry->builtin ? @"Built-in Display" : [NSString stringWithFormat:@"Display %d", ++external];
            }
            vp_copy_string(name, entry->name, sizeof entry->name);
            written++;
        }
        return written;
    }
}

/* ============================================================================================ */
/* Permissions                                                                                  */
/* ============================================================================================ */

bool vp_screen_capture_allowed(void) { return CGPreflightScreenCaptureAccess(); }

bool vp_screen_capture_request(void) { return CGRequestScreenCaptureAccess(); }

bool vp_accessibility_allowed(bool prompt) {
    @autoreleasepool {
        NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt : @(prompt)};
        return AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
    }
}

void vp_open_privacy_settings(int32_t pane) {
    @autoreleasepool {
        NSString *anchor = pane == 1 ? @"Privacy_Accessibility" : pane == 2 ? @"Privacy_LocalNetwork" : @"Privacy_ScreenCapture";
        NSString *address = [@"x-apple.systempreferences:com.apple.preference.security?" stringByAppendingString:anchor];
        NSURL *url = [NSURL URLWithString:address];
        if (url) [[NSWorkspace sharedWorkspace] openURL:url];
    }
}

/* ============================================================================================ */
/* Capture                                                                                      */
/* ============================================================================================ */

@interface VPCapture : NSObject <SCStreamOutput, SCStreamDelegate>
@property(nonatomic, strong) SCStream *stream;
@property(nonatomic, strong) dispatch_queue_t queue;
@property(nonatomic, strong) NSCondition *condition;
@property(nonatomic, assign) CVPixelBufferRef latest;
@property(nonatomic, assign) BOOL fresh;
@property(nonatomic, assign) BOOL stopped;
@end

@implementation VPCapture

- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)sampleBuffer ofType:(SCStreamOutputType)type {
    if (type != SCStreamOutputTypeScreen || !CMSampleBufferIsValid(sampleBuffer)) return;

    /* ScreenCaptureKit also emits "idle" and "blank" samples; only complete frames carry pixels. */
    CFArrayRef attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, false);
    if (attachments && CFArrayGetCount(attachments) > 0) {
        NSDictionary *info = (__bridge NSDictionary *)CFArrayGetValueAtIndex(attachments, 0);
        NSNumber *status = info[SCStreamFrameInfoStatus];
        if (status && status.integerValue != SCFrameStatusComplete) return;
    }
    CVPixelBufferRef pixels = CMSampleBufferGetImageBuffer(sampleBuffer);
    if (!pixels) return;

    [self.condition lock];
    if (!self.stopped) {
        if (self.latest) CVPixelBufferRelease(self.latest);
        self.latest = CVPixelBufferRetain(pixels);
        self.fresh = YES;
        [self.condition signal];
    }
    [self.condition unlock];
}

- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
    [self.condition lock];
    self.stopped = YES;
    [self.condition broadcast];
    [self.condition unlock];
}

- (void)dealloc {
    if (_latest) {
        CVPixelBufferRelease(_latest);
        _latest = NULL;
    }
}

@end

vp_capture_ref vp_capture_start(uint32_t display_id, uint32_t width, uint32_t height, uint32_t fps,
                                char *error, size_t error_len) {
    @autoreleasepool {
        if (!CGPreflightScreenCaptureAccess()) {
            vp_error(error, error_len, @"Screen Recording permission is required (System Settings → Privacy & Security → Screen & System Audio Recording).");
            return NULL;
        }

        __block SCShareableContent *content = nil;
        __block NSError *contentError = nil;
        dispatch_semaphore_t loaded = dispatch_semaphore_create(0);
        [SCShareableContent getShareableContentExcludingDesktopWindows:NO
                                                   onScreenWindowsOnly:YES
                                                     completionHandler:^(SCShareableContent *shareable, NSError *failure) {
                                                         content = shareable;
                                                         contentError = failure;
                                                         dispatch_semaphore_signal(loaded);
                                                     }];
        if (!vp_wait(loaded, 5)) {
            vp_error(error, error_len, @"ScreenCaptureKit did not answer in time.");
            return NULL;
        }
        if (!content) {
            vp_error(error, error_len, @"Screen capture is unavailable: %@", contentError.localizedDescription ?: @"unknown error");
            return NULL;
        }

        SCDisplay *target = nil;
        for (SCDisplay *display in content.displays) {
            if (display.displayID == display_id) {
                target = display;
                break;
            }
        }
        if (!target) {
            vp_error(error, error_len, @"Display %u is no longer available.", display_id);
            return NULL;
        }

        SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:target excludingWindows:@[]];
        SCStreamConfiguration *config = [[SCStreamConfiguration alloc] init];
        /* The window server scales and converts on the GPU: frames arrive ready for the encoder. */
        config.width = width;
        config.height = height;
        config.minimumFrameInterval = CMTimeMake(1, (int32_t)MAX(fps, 1u));
        config.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
        config.colorMatrix = kCGDisplayStreamYCbCrMatrix_ITU_R_709_2;
        config.colorSpaceName = kCGColorSpaceSRGB;
        /* The iPad draws the pointer locally from position + shape updates, without video latency. */
        config.showsCursor = NO;
        config.queueDepth = 5;
        config.capturesAudio = NO;

        VPCapture *capture = [[VPCapture alloc] init];
        dispatch_queue_attr_t attributes =
            dispatch_queue_attr_make_with_qos_class(DISPATCH_QUEUE_SERIAL, QOS_CLASS_USER_INTERACTIVE, 0);
        capture.queue = dispatch_queue_create("com.vision-pro.capture", attributes);
        capture.condition = [[NSCondition alloc] init];
        capture.stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:capture];

        NSError *outputError = nil;
        if (![capture.stream addStreamOutput:capture type:SCStreamOutputTypeScreen sampleHandlerQueue:capture.queue error:&outputError]) {
            vp_error(error, error_len, @"Screen capture output rejected: %@", outputError.localizedDescription ?: @"unknown error");
            return NULL;
        }

        __block NSError *startError = nil;
        dispatch_semaphore_t started = dispatch_semaphore_create(0);
        [capture.stream startCaptureWithCompletionHandler:^(NSError *failure) {
            startError = failure;
            dispatch_semaphore_signal(started);
        }];
        if (!vp_wait(started, 5)) {
            vp_error(error, error_len, @"Screen capture did not start in time.");
            return NULL;
        }
        if (startError) {
            vp_error(error, error_len, @"Screen capture could not start: %@", startError.localizedDescription);
            return NULL;
        }
        return (__bridge_retained void *)capture;
    }
}

int32_t vp_capture_next(vp_capture_ref ref, uint32_t timeout_ms, vp_frame_ref *frame) {
    if (!ref || !frame) return -1;
    VPCapture *capture = (__bridge VPCapture *)ref;
    int32_t result = 0;
    @autoreleasepool {
        NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:timeout_ms / 1000.0];
        [capture.condition lock];
        while (!capture.fresh && !capture.stopped) {
            if (![capture.condition waitUntilDate:deadline]) break;
        }
        if (capture.stopped) {
            result = -1;
        } else if (capture.fresh && capture.latest) {
            *frame = CVPixelBufferRetain(capture.latest);
            capture.fresh = NO;
            result = 1;
        }
        [capture.condition unlock];
    }
    return result;
}

void vp_capture_stop(vp_capture_ref ref) {
    if (!ref) return;
    VPCapture *capture = (__bridge_transfer VPCapture *)ref;
    @autoreleasepool {
        [capture.condition lock];
        capture.stopped = YES;
        [capture.condition broadcast];
        [capture.condition unlock];

        dispatch_semaphore_t stopped = dispatch_semaphore_create(0);
        [capture.stream stopCaptureWithCompletionHandler:^(NSError *failure) {
            dispatch_semaphore_signal(stopped);
        }];
        vp_wait(stopped, 2);
        [capture.stream removeStreamOutput:capture type:SCStreamOutputTypeScreen error:nil];
        /* Drain a sample callback that may still be running before the object goes away. */
        dispatch_sync(capture.queue, ^{
                      });
        capture.stream = nil;

        [capture.condition lock];
        if (capture.latest) {
            CVPixelBufferRelease(capture.latest);
            capture.latest = NULL;
        }
        [capture.condition unlock];
    }
}

vp_frame_ref vp_frame_retain(vp_frame_ref frame) {
    return frame ? (vp_frame_ref)CVPixelBufferRetain((CVPixelBufferRef)frame) : NULL;
}

void vp_frame_release(vp_frame_ref frame) {
    if (frame) CVPixelBufferRelease((CVPixelBufferRef)frame);
}

bool vp_frame_copy_nv12(vp_frame_ref frame, uint8_t *y, uint8_t *uv, uint32_t width, uint32_t height) {
    CVPixelBufferRef pixels = (CVPixelBufferRef)frame;
    if (!pixels || !y || !uv || CVPixelBufferGetPlaneCount(pixels) < 2) return false;
    if (CVPixelBufferGetWidth(pixels) < width || CVPixelBufferGetHeight(pixels) < height) return false;
    if (CVPixelBufferLockBaseAddress(pixels, kCVPixelBufferLock_ReadOnly) != kCVReturnSuccess) return false;

    const uint8_t *luma = CVPixelBufferGetBaseAddressOfPlane(pixels, 0);
    const uint8_t *chroma = CVPixelBufferGetBaseAddressOfPlane(pixels, 1);
    size_t luma_stride = CVPixelBufferGetBytesPerRowOfPlane(pixels, 0);
    size_t chroma_stride = CVPixelBufferGetBytesPerRowOfPlane(pixels, 1);
    bool ok = luma && chroma && luma_stride >= width && chroma_stride >= width;
    if (ok) {
        for (uint32_t row = 0; row < height; row++) memcpy(y + (size_t)row * width, luma + row * luma_stride, width);
        for (uint32_t row = 0; row < height / 2; row++) memcpy(uv + (size_t)row * width, chroma + row * chroma_stride, width);
    }
    CVPixelBufferUnlockBaseAddress(pixels, kCVPixelBufferLock_ReadOnly);
    return ok;
}

/* ============================================================================================ */
/* VideoToolbox encoder                                                                         */
/* ============================================================================================ */

@interface VPEncoder : NSObject {
  @public
    VTCompressionSessionRef session;
    CMVideoCodecType codec;
    NSMutableData *output;
    BOOL keyframe;
    BOOL hardware;
    OSStatus callbackStatus;
}
@end

@implementation VPEncoder
- (void)dealloc {
    if (session) {
        VTCompressionSessionInvalidate(session);
        CFRelease(session);
        session = NULL;
    }
}
@end

static void vp_append_nal(NSMutableData *out, const uint8_t *bytes, size_t length) {
    static const uint8_t start_code[4] = {0, 0, 0, 1};
    [out appendBytes:start_code length:4];
    [out appendBytes:bytes length:length];
}

/* Converts VideoToolbox's length-prefixed (AVCC / HVCC) output into an Annex B access unit, with
 * the parameter sets in front of every keyframe so the iPad can join or recover at any IDR. */
static void vp_encoder_output(void *refcon, void *frame_refcon, OSStatus status, VTEncodeInfoFlags flags,
                              CMSampleBufferRef sample) {
    VPEncoder *encoder = (__bridge VPEncoder *)refcon;
    if (status != noErr) {
        encoder->callbackStatus = status;
        return;
    }
    if (!sample || (flags & kVTEncodeInfo_FrameDropped)) return;

    BOOL key = YES;
    CFArrayRef attachments = CMSampleBufferGetSampleAttachmentsArray(sample, false);
    if (attachments && CFArrayGetCount(attachments) > 0) {
        CFDictionaryRef attachment = (CFDictionaryRef)CFArrayGetValueAtIndex(attachments, 0);
        const void *not_sync = NULL;
        if (CFDictionaryGetValueIfPresent(attachment, kCMSampleAttachmentKey_NotSync, &not_sync) && not_sync &&
            CFBooleanGetValue((CFBooleanRef)not_sync)) {
            key = NO;
        }
    }

    int header_length = 4;
    CMFormatDescriptionRef format = CMSampleBufferGetFormatDescription(sample);
    if (format) {
        BOOL hevc = encoder->codec == kCMVideoCodecType_HEVC;
        size_t count = 0;
        OSStatus described = hevc
            ? CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(format, 0, NULL, NULL, &count, &header_length)
            : CMVideoFormatDescriptionGetH264ParameterSetAtIndex(format, 0, NULL, NULL, &count, &header_length);
        if (described != noErr) {
            count = 0;
            header_length = 4;
        }
        if (key) {
            for (size_t index = 0; index < count; index++) {
                const uint8_t *set = NULL;
                size_t set_length = 0;
                OSStatus got = hevc
                    ? CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(format, index, &set, &set_length, NULL, NULL)
                    : CMVideoFormatDescriptionGetH264ParameterSetAtIndex(format, index, &set, &set_length, NULL, NULL);
                if (got == noErr && set && set_length) vp_append_nal(encoder->output, set, set_length);
            }
        }
    }
    if (header_length < 1 || header_length > 4) return;

    CMBlockBufferRef block = CMSampleBufferGetDataBuffer(sample);
    if (!block) return;
    size_t length = CMBlockBufferGetDataLength(block);
    if (length == 0) return;

    NSMutableData *copy = nil;
    char *pointer = NULL;
    size_t contiguous = 0;
    size_t total = 0;
    if (CMBlockBufferGetDataPointer(block, 0, &contiguous, &total, &pointer) != kCMBlockBufferNoErr || contiguous < length) {
        copy = [NSMutableData dataWithLength:length];
        if (CMBlockBufferCopyDataBytes(block, 0, length, copy.mutableBytes) != kCMBlockBufferNoErr) return;
        pointer = copy.mutableBytes;
    }

    const uint8_t *bytes = (const uint8_t *)pointer;
    size_t offset = 0;
    while (offset + (size_t)header_length <= length) {
        uint32_t nal_length = 0;
        for (int k = 0; k < header_length; k++) nal_length = (nal_length << 8) | bytes[offset + k];
        offset += header_length;
        if (nal_length == 0 || offset + nal_length > length) break;
        vp_append_nal(encoder->output, bytes + offset, nal_length);
        offset += nal_length;
    }
    if (key) encoder->keyframe = YES;
}

static void vp_apply_bitrate(VTCompressionSessionRef session, uint32_t bitrate_kbps) {
    int64_t bps = (int64_t)bitrate_kbps * 1000;
    VTSessionSetProperty(session, kVTCompressionPropertyKey_AverageBitRate, (__bridge CFTypeRef) @(bps));
    /* A quarter-second window with 50% headroom: keyframes cannot park a second of already stale
     * desktop video in the Wi-Fi queue, which is what the user perceives as input lag. */
    NSArray *limits = @[ @(bps * 3 / 64), @(0.25) ];
    VTSessionSetProperty(session, kVTCompressionPropertyKey_DataRateLimits, (__bridge CFArrayRef)limits);
}

uint32_t vp_hardware_encoders(void) {
    uint32_t mask = 0;
    @autoreleasepool {
        const CMVideoCodecType codecs[2] = {kCMVideoCodecType_H264, kCMVideoCodecType_HEVC};
        NSDictionary *spec = @{(__bridge NSString *)kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder : @YES};
        for (int i = 0; i < 2; i++) {
            VTCompressionSessionRef session = NULL;
            OSStatus status = VTCompressionSessionCreate(kCFAllocatorDefault, 1280, 720, codecs[i], (__bridge CFDictionaryRef)spec,
                                                         NULL, kCFAllocatorDefault, NULL, NULL, &session);
            if (status == noErr && session) {
                mask |= 1u << i;
                VTCompressionSessionInvalidate(session);
                CFRelease(session);
            }
        }
    }
    return mask;
}

vp_encoder_ref vp_encoder_create(int32_t codec, uint32_t width, uint32_t height, uint32_t fps,
                                 uint32_t bitrate_kbps, bool prioritize_speed, char *error,
                                 size_t error_len) {
    @autoreleasepool {
        VPEncoder *encoder = [[VPEncoder alloc] init];
        encoder->codec = codec == VP_CODEC_HEVC ? kCMVideoCodecType_HEVC : kCMVideoCodecType_H264;
        encoder->output = [NSMutableData dataWithCapacity:1 << 20];

        NSDictionary *source = @{
            (__bridge NSString *)kCVPixelBufferPixelFormatTypeKey : @(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange),
            (__bridge NSString *)kCVPixelBufferWidthKey : @(width),
            (__bridge NSString *)kCVPixelBufferHeightKey : @(height),
            (__bridge NSString *)kCVPixelBufferIOSurfacePropertiesKey : @{},
        };

        OSStatus status = kVTParameterErr;
        /* First try the low-latency rate controller (no frame buffering inside the encoder),
         * then the regular real-time hardware configuration. */
        for (int attempt = 0; attempt < 2 && status != noErr; attempt++) {
            NSMutableDictionary *spec = [NSMutableDictionary dictionary];
            spec[(__bridge NSString *)kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder] = @YES;
            if (attempt == 0) spec[(__bridge NSString *)kVTVideoEncoderSpecification_EnableLowLatencyRateControl] = @YES;
            VTCompressionSessionRef session = NULL;
            status = VTCompressionSessionCreate(kCFAllocatorDefault, (int32_t)width, (int32_t)height, encoder->codec,
                                                (__bridge CFDictionaryRef)spec, (__bridge CFDictionaryRef)source,
                                                kCFAllocatorDefault, vp_encoder_output, (__bridge void *)encoder, &session);
            if (status == noErr) encoder->session = session;
        }
        if (status != noErr || !encoder->session) {
            vp_error(error, error_len, @"VideoToolbox has no hardware %@ encoder for %ux%u (OSStatus %d).",
                     codec == VP_CODEC_HEVC ? @"HEVC" : @"H.264", width, height, (int)status);
            return NULL;
        }

        VTCompressionSessionRef session = encoder->session;
        VTSessionSetProperty(session, kVTCompressionPropertyKey_RealTime, kCFBooleanTrue);
        VTSessionSetProperty(session, kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse);
        if (encoder->codec == kCMVideoCodecType_H264) {
            VTSessionSetProperty(session, kVTCompressionPropertyKey_ProfileLevel, kVTProfileLevel_H264_High_AutoLevel);
            VTSessionSetProperty(session, kVTCompressionPropertyKey_H264EntropyMode, kVTH264EntropyMode_CABAC);
        } else {
            VTSessionSetProperty(session, kVTCompressionPropertyKey_ProfileLevel, kVTProfileLevel_HEVC_Main_AutoLevel);
        }
        VTSessionSetProperty(session, kVTCompressionPropertyKey_ExpectedFrameRate, (__bridge CFTypeRef) @(fps));
        /* Keyframes are produced on demand (join, loss, congestion); the periodic one is a safety net. */
        VTSessionSetProperty(session, kVTCompressionPropertyKey_MaxKeyFrameInterval, (__bridge CFTypeRef) @(fps * 30));
        VTSessionSetProperty(session, kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration, (__bridge CFTypeRef) @(30));
        VTSessionSetProperty(session, kVTCompressionPropertyKey_ColorPrimaries, kCVImageBufferColorPrimaries_ITU_R_709_2);
        VTSessionSetProperty(session, kVTCompressionPropertyKey_TransferFunction, kCVImageBufferTransferFunction_ITU_R_709_2);
        VTSessionSetProperty(session, kVTCompressionPropertyKey_YCbCrMatrix, kCVImageBufferYCbCrMatrix_ITU_R_709_2);
        if (prioritize_speed) {
            if (@available(macOS 13.0, *)) {
                VTSessionSetProperty(session, kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality, kCFBooleanTrue);
            }
        }
        vp_apply_bitrate(session, bitrate_kbps);

        CFBooleanRef using_hardware = NULL;
        if (VTSessionCopyProperty(session, kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder, kCFAllocatorDefault,
                                  &using_hardware) == noErr &&
            using_hardware) {
            encoder->hardware = CFBooleanGetValue(using_hardware);
            CFRelease(using_hardware);
        } else {
            encoder->hardware = YES;
        }
        VTCompressionSessionPrepareToEncodeFrames(session);
        return (__bridge_retained void *)encoder;
    }
}

void vp_encoder_set_bitrate(vp_encoder_ref ref, uint32_t bitrate_kbps) {
    if (!ref) return;
    VPEncoder *encoder = (__bridge VPEncoder *)ref;
    @autoreleasepool {
        vp_apply_bitrate(encoder->session, bitrate_kbps);
    }
}

int32_t vp_encoder_encode(vp_encoder_ref ref, vp_frame_ref frame, int64_t pts_us, bool force_keyframe,
                          const uint8_t **data, size_t *len, bool *keyframe) {
    if (!ref || !frame || !data || !len || !keyframe) return kVTParameterErr;
    VPEncoder *encoder = (__bridge VPEncoder *)ref;
    OSStatus status;
    @autoreleasepool {
        [encoder->output setLength:0];
        encoder->keyframe = NO;
        encoder->callbackStatus = noErr;
        CMTime pts = CMTimeMake(pts_us, 1000000);
        NSDictionary *options = force_keyframe ? @{(__bridge NSString *)kVTEncodeFrameOptionKey_ForceKeyFrame : @YES} : nil;
        status = VTCompressionSessionEncodeFrame(encoder->session, (CVImageBufferRef)frame, pts, kCMTimeInvalid,
                                                 (__bridge CFDictionaryRef)options, NULL, NULL);
        /* Real-time mode without B-frames: flushing makes the call synchronous with one frame in flight. */
        if (status == noErr) status = VTCompressionSessionCompleteFrames(encoder->session, pts);
        if (status == noErr) status = encoder->callbackStatus;
    }
    *data = encoder->output.bytes;
    *len = encoder->output.length;
    *keyframe = encoder->keyframe;
    return status;
}

bool vp_encoder_is_hardware(vp_encoder_ref ref) {
    return ref ? ((__bridge VPEncoder *)ref)->hardware : false;
}

void vp_encoder_destroy(vp_encoder_ref ref) {
    if (!ref) return;
    VPEncoder *encoder = (__bridge_transfer VPEncoder *)ref;
    (void)encoder; /* ARC releases it here; -dealloc invalidates the session. */
}

/* ============================================================================================ */
/* Cursor                                                                                       */
/* ============================================================================================ */

bool vp_cursor_location(double *x, double *y) {
    CGEventRef event = CGEventCreate(NULL);
    if (!event) return false;
    CGPoint point = CGEventGetLocation(event);
    CFRelease(event);
    if (x) *x = point.x;
    if (y) *y = point.y;
    return true;
}

int32_t vp_cursor_image(double scale, uint8_t *rgba, size_t capacity, vp_cursor_info_t *info) {
    if (!rgba || !info) return 0;
    @autoreleasepool {
        @try {
            NSCursor *cursor = [NSCursor currentSystemCursor];
            if (!cursor) return 0;
            NSImage *image = cursor.image;
            NSSize size = image.size;
            if (size.width <= 0 || size.height <= 0) return 0;
            if (!(scale >= 1)) scale = 1;
            if (scale > 4) scale = 4;
            while ((size.width * scale > 256 || size.height * scale > 256) && scale > 0.25) scale /= 2;

            size_t width = (size_t)ceil(size.width * scale);
            size_t height = (size_t)ceil(size.height * scale);
            if (width == 0 || height == 0) return 0;
            if (width * height * 4 > capacity) return -1;

            NSRect proposed = NSMakeRect(0, 0, (CGFloat)width, (CGFloat)height);
            CGImageRef cg = [image CGImageForProposedRect:&proposed context:nil hints:nil];
            if (!cg) return 0;

            memset(rgba, 0, width * height * 4);
            CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
            CGContextRef context = CGBitmapContextCreate(rgba, width, height, 8, width * 4, space,
                                                         (CGBitmapInfo)kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
            CGColorSpaceRelease(space);
            if (!context) return 0;
            CGContextSetInterpolationQuality(context, kCGInterpolationHigh);
            CGContextDrawImage(context, CGRectMake(0, 0, (CGFloat)width, (CGFloat)height), cg);
            CGContextRelease(context);

            uint64_t hash = 1469598103934665603ULL;
            for (size_t i = 0; i < width * height * 4; i += 4) {
                uint8_t alpha = rgba[i + 3];
                if (alpha && alpha < 255) {
                    for (int c = 0; c < 3; c++) rgba[i + c] = (uint8_t)MIN(255, (rgba[i + c] * 255 + alpha / 2) / alpha);
                }
                for (int c = 0; c < 4; c++) {
                    hash ^= rgba[i + c];
                    hash *= 1099511628211ULL;
                }
            }
            NSPoint hot = cursor.hotSpot;
            info->width = (uint32_t)width;
            info->height = (uint32_t)height;
            info->hot_x = (int32_t)lround(hot.x * scale);
            info->hot_y = (int32_t)lround(hot.y * scale);
            info->hash = hash ^ ((uint64_t)width << 48) ^ ((uint64_t)height << 32);
            return 1;
        } @catch (NSException *exception) {
            return 0;
        }
    }
}

/* ============================================================================================ */
/* Input injection                                                                              */
/* ============================================================================================ */

static CGEventSourceRef vp_source(void) {
    static CGEventSourceRef source = NULL;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        source = CGEventSourceCreate(kCGEventSourceStateHIDSystemState);
        /* Keep the Mac's own keyboard and trackpad responsive while the iPad is injecting. */
        if (source) CGEventSourceSetLocalEventsSuppressionInterval(source, 0.0);
    });
    return source;
}

/* Only the dedicated input thread injects, so this state needs no locking. */
static CGPoint vp_last_point;
static bool vp_has_last_point = false;
static double vp_delta_rest_x = 0, vp_delta_rest_y = 0;

static void vp_post(CGEventRef event, uint64_t flags) {
    if (!event) return;
    CGEventSetFlags(event, (CGEventFlags)(flags | kCGEventFlagMaskNonCoalesced));
    CGEventPost(kCGHIDEventTap, event);
    CFRelease(event);
}

/* Relative deltas matter to 3D viewports and games that read NSEvent.deltaX/Y. */
static void vp_set_delta(CGEventRef event, CGPoint point) {
    if (vp_has_last_point) {
        double dx = point.x - vp_last_point.x + vp_delta_rest_x;
        double dy = point.y - vp_last_point.y + vp_delta_rest_y;
        int64_t ix = (int64_t)trunc(dx);
        int64_t iy = (int64_t)trunc(dy);
        vp_delta_rest_x = dx - (double)ix;
        vp_delta_rest_y = dy - (double)iy;
        CGEventSetIntegerValueField(event, kCGMouseEventDeltaX, ix);
        CGEventSetIntegerValueField(event, kCGMouseEventDeltaY, iy);
    }
    vp_last_point = point;
    vp_has_last_point = true;
}

static CGMouseButton vp_cg_button(int32_t button) {
    switch (button) {
        case VP_BUTTON_LEFT: return kCGMouseButtonLeft;
        case VP_BUTTON_RIGHT: return kCGMouseButtonRight;
        default: return kCGMouseButtonCenter;
    }
}

double vp_double_click_interval(void) {
    @autoreleasepool {
        double interval = [NSEvent doubleClickInterval];
        return interval > 0 ? interval : 0.5;
    }
}

void vp_mouse_move(double x, double y, int32_t held_button, uint64_t flags) {
    CGEventType type = kCGEventMouseMoved;
    if (held_button == VP_BUTTON_LEFT) type = kCGEventLeftMouseDragged;
    else if (held_button == VP_BUTTON_RIGHT) type = kCGEventRightMouseDragged;
    else if (held_button >= VP_BUTTON_MIDDLE) type = kCGEventOtherMouseDragged;
    CGPoint point = CGPointMake(x, y);
    CGEventRef event = CGEventCreateMouseEvent(vp_source(), type, point, held_button < 0 ? kCGMouseButtonLeft : vp_cg_button(held_button));
    if (!event) return;
    if (held_button >= VP_BUTTON_MIDDLE) CGEventSetIntegerValueField(event, kCGMouseEventButtonNumber, held_button);
    vp_set_delta(event, point);
    vp_post(event, flags);
}

void vp_mouse_button(double x, double y, int32_t button, bool down, int32_t click_count, uint64_t flags) {
    CGEventType type;
    if (button == VP_BUTTON_LEFT) type = down ? kCGEventLeftMouseDown : kCGEventLeftMouseUp;
    else if (button == VP_BUTTON_RIGHT) type = down ? kCGEventRightMouseDown : kCGEventRightMouseUp;
    else type = down ? kCGEventOtherMouseDown : kCGEventOtherMouseUp;
    CGPoint point = CGPointMake(x, y);
    CGEventRef event = CGEventCreateMouseEvent(vp_source(), type, point, vp_cg_button(button));
    if (!event) return;
    if (button >= VP_BUTTON_MIDDLE) CGEventSetIntegerValueField(event, kCGMouseEventButtonNumber, button);
    /* Double and triple clicks are recognised by macOS only through the click state field. */
    CGEventSetIntegerValueField(event, kCGMouseEventClickState, click_count < 1 ? 1 : click_count);
    vp_last_point = point;
    vp_has_last_point = true;
    vp_post(event, flags);
}

void vp_scroll(int32_t vertical, int32_t horizontal, bool lines, uint64_t flags) {
    if (vertical == 0 && horizontal == 0) return;
    CGEventRef event = CGEventCreateScrollWheelEvent2(vp_source(), lines ? kCGScrollEventUnitLine : kCGScrollEventUnitPixel, 2,
                                                      vertical, horizontal, 0);
    if (!event) return;
    if (!lines) CGEventSetIntegerValueField(event, kCGScrollWheelEventIsContinuous, 1);
    vp_post(event, flags);
}

void vp_key(uint16_t keycode, bool down, uint64_t flags, bool modifier) {
    CGEventRef event = CGEventCreateKeyboardEvent(vp_source(), (CGKeyCode)keycode, down);
    if (!event) return;
    /* Modifier keys are "flags changed" events on macOS; the flags carry the new state. */
    if (modifier) CGEventSetType(event, kCGEventFlagsChanged);
    vp_post(event, flags);
}

void vp_text(const uint16_t *units, size_t count) {
    if (!units) return;
    size_t offset = 0;
    while (offset < count) {
        size_t chunk = MIN(count - offset, (size_t)16);
        /* Never split a surrogate pair between two events. */
        if (chunk > 1 && offset + chunk < count && CFStringIsSurrogateHighCharacter(units[offset + chunk - 1])) chunk--;
        for (int down = 1; down >= 0; down--) {
            CGEventRef event = CGEventCreateKeyboardEvent(vp_source(), 0, down == 1);
            if (!event) return;
            CGEventKeyboardSetUnicodeString(event, (UniCharCount)chunk, (const UniChar *)(units + offset));
            /* Typed text never carries held modifiers: it must not turn into shortcuts. */
            CGEventSetFlags(event, kCGEventFlagMaskNonCoalesced);
            CGEventPost(kCGHIDEventTap, event);
            CFRelease(event);
        }
        offset += chunk;
    }
}

void vp_media_key(int32_t key, bool down) {
    @autoreleasepool {
        NSInteger state = down ? 0xa : 0xb;
        NSInteger data1 = ((NSInteger)key << 16) | (state << 8);
        NSEvent *event = [NSEvent otherEventWithType:NSEventTypeSystemDefined
                                            location:NSZeroPoint
                                       modifierFlags:(NSEventModifierFlags)(state << 8)
                                           timestamp:0
                                        windowNumber:0
                                             context:nil
                                             subtype:8
                                               data1:data1
                                               data2:-1];
        CGEventRef cg = event.CGEvent;
        if (cg) CGEventPost(kCGHIDEventTap, cg);
    }
}

/* Tablet identity announced in proximity events (pen or eraser end of the same stylus). */
enum {
    VP_TABLET_DEVICE_ID = 0x5650,
    VP_POINTER_PEN = 1,    /* NX_TABLET_POINTER_PEN */
    VP_POINTER_ERASER = 3, /* NX_TABLET_POINTER_ERASER */
    /* NX_TABLET_CAPABILITY_* : device id, abs x/y, buttons, tilt x/y, pressure, rotation. */
    VP_TABLET_CAPABILITIES = 0x0001 | 0x0002 | 0x0004 | 0x0040 | 0x0080 | 0x0100 | 0x0400 | 0x2000,
};

void vp_tablet_proximity(bool entering, bool eraser) {
    CGPoint point = vp_last_point;
    if (!vp_has_last_point) {
        double x = 0, y = 0;
        vp_cursor_location(&x, &y);
        point = CGPointMake(x, y);
    }
    CGEventRef event = CGEventCreateMouseEvent(vp_source(), kCGEventMouseMoved, point, kCGMouseButtonLeft);
    if (!event) return;
    CGEventSetType(event, kCGEventTabletProximity);
    CGEventSetIntegerValueField(event, kCGMouseEventSubtype, kCGEventMouseSubtypeTabletProximity);
    CGEventSetIntegerValueField(event, kCGTabletProximityEventTabletID, 1);
    CGEventSetIntegerValueField(event, kCGTabletProximityEventSystemTabletID, 1);
    CGEventSetIntegerValueField(event, kCGTabletProximityEventPointerID, eraser ? 2 : 1);
    CGEventSetIntegerValueField(event, kCGTabletProximityEventDeviceID, VP_TABLET_DEVICE_ID);
    CGEventSetIntegerValueField(event, kCGTabletProximityEventVendorPointerType, eraser ? 0x080A : 0x0802);
    CGEventSetIntegerValueField(event, kCGTabletProximityEventVendorPointerSerialNumber, 1);
    CGEventSetIntegerValueField(event, kCGTabletProximityEventCapabilityMask, VP_TABLET_CAPABILITIES);
    CGEventSetIntegerValueField(event, kCGTabletProximityEventPointerType, eraser ? VP_POINTER_ERASER : VP_POINTER_PEN);
    CGEventSetIntegerValueField(event, kCGTabletProximityEventEnterProximity, entering ? 1 : 0);
    vp_post(event, 0);
}

void vp_tablet_point(double x, double y, int32_t phase, int32_t button, double pressure, double tilt_x, double tilt_y,
                     double rotation, uint64_t flags) {
    bool right = button == VP_BUTTON_RIGHT;
    CGEventType type;
    switch (phase) {
        case 1: type = right ? kCGEventRightMouseDown : kCGEventLeftMouseDown; break;
        case 2: type = right ? kCGEventRightMouseDragged : kCGEventLeftMouseDragged; break;
        case 3: type = right ? kCGEventRightMouseUp : kCGEventLeftMouseUp; break;
        default: type = kCGEventMouseMoved; break;
    }
    CGPoint point = CGPointMake(x, y);
    CGEventRef event = CGEventCreateMouseEvent(vp_source(), type, point, right ? kCGMouseButtonRight : kCGMouseButtonLeft);
    if (!event) return;
    vp_set_delta(event, point);
    bool contact = phase == 1 || phase == 2;
    double p = contact ? fmin(1.0, fmax(0.0, pressure)) : 0.0;
    CGEventSetIntegerValueField(event, kCGMouseEventSubtype, kCGEventMouseSubtypeTabletPoint);
    CGEventSetDoubleValueField(event, kCGMouseEventPressure, p);
    CGEventSetIntegerValueField(event, kCGTabletEventPointPressure, (int64_t)llround(p * 65535.0));
    CGEventSetDoubleValueField(event, kCGTabletEventTiltX, fmin(1.0, fmax(-1.0, tilt_x)));
    CGEventSetDoubleValueField(event, kCGTabletEventTiltY, fmin(1.0, fmax(-1.0, tilt_y)));
    CGEventSetDoubleValueField(event, kCGTabletEventRotation, rotation);
    CGEventSetIntegerValueField(event, kCGTabletEventDeviceID, VP_TABLET_DEVICE_ID);
    CGEventSetIntegerValueField(event, kCGTabletEventPointButtons, contact ? 1 : 0);
    if (phase == 1 || phase == 3) CGEventSetIntegerValueField(event, kCGMouseEventClickState, 1);
    vp_post(event, flags);
}

/* Trackpad gestures are not part of the public Quartz API. These WindowServer event fields are the
 * long-stable ones also used by Hammerspoon (hs.eventtap.event.newGesture). If a future macOS
 * ignores them, pinch simply does nothing and the iPad's local zoom remains available. */
enum {
    VP_EVENT_GESTURE = 29,
    VP_FIELD_GESTURE_HID_TYPE = 110,
    VP_FIELD_GESTURE_ZOOM_VALUE = 113,
    VP_FIELD_GESTURE_PHASE = 132,
    VP_HID_ZOOM = 8,
    VP_HID_GESTURE_BEGIN = 61,
    VP_HID_GESTURE_END = 62,
};

static void vp_gesture_event(int64_t hid_type, int64_t phase, double magnification, CGPoint point) {
    CGEventRef event = CGEventCreate(vp_source());
    if (!event) return;
    CGEventSetType(event, (CGEventType)VP_EVENT_GESTURE);
    CGEventSetLocation(event, point);
    CGEventSetIntegerValueField(event, (CGEventField)VP_FIELD_GESTURE_HID_TYPE, hid_type);
    if (hid_type == VP_HID_ZOOM) {
        CGEventSetDoubleValueField(event, (CGEventField)VP_FIELD_GESTURE_ZOOM_VALUE, magnification);
        CGEventSetIntegerValueField(event, (CGEventField)VP_FIELD_GESTURE_PHASE, phase);
    }
    CGEventPost(kCGHIDEventTap, event);
    CFRelease(event);
}

void vp_magnify(int32_t phase, double magnification, double x, double y) {
    CGPoint point = CGPointMake(x, y);
    switch (phase) {
        case 0:
            vp_gesture_event(VP_HID_GESTURE_BEGIN, 0, 0, point);
            vp_gesture_event(VP_HID_ZOOM, 1 /* began */, 0, point);
            break;
        case 1:
            vp_gesture_event(VP_HID_ZOOM, 2 /* changed */, magnification, point);
            break;
        default:
            vp_gesture_event(VP_HID_ZOOM, 4 /* ended */, 0, point);
            vp_gesture_event(VP_HID_GESTURE_END, 0, 0, point);
            break;
    }
}

/* ============================================================================================ */
/* System                                                                                       */
/* ============================================================================================ */

bool vp_frontmost_app(char *bundle_id, size_t bundle_len, char *name, size_t name_len) {
    @autoreleasepool {
        /* The window list is ordered front to back and, unlike NSWorkspace, stays current in
         * processes that do not run an AppKit event loop (the headless agent). */
        pid_t pid = 0;
        CFArrayRef windows = CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
                                                        kCGNullWindowID);
        if (windows) {
            for (NSDictionary *window in (__bridge NSArray *)windows) {
                NSNumber *layer = window[(__bridge NSString *)kCGWindowLayer];
                NSNumber *owner = window[(__bridge NSString *)kCGWindowOwnerPID];
                if (!owner || layer.integerValue != 0) continue;
                NSDictionary *bounds = window[(__bridge NSString *)kCGWindowBounds];
                CGRect rect = CGRectZero;
                if (bounds && CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)bounds, &rect) &&
                    (rect.size.width < 48 || rect.size.height < 48)) {
                    continue;
                }
                pid = owner.intValue;
                break;
            }
            CFRelease(windows);
        }
        NSRunningApplication *app = pid > 0 ? [NSRunningApplication runningApplicationWithProcessIdentifier:pid] : nil;
        if (!app) app = [NSWorkspace sharedWorkspace].frontmostApplication;
        if (!app) return false;
        NSString *identifier = app.bundleIdentifier ?: app.executableURL.lastPathComponent ?: @"";
        if (!identifier.length) return false;
        vp_copy_string(identifier, bundle_id, bundle_len);
        vp_copy_string(app.localizedName ?: identifier, name, name_len);
        return true;
    }
}

bool vp_computer_name(char *out, size_t len) {
    @autoreleasepool {
        NSString *name = [[NSHost currentHost] localizedName];
        if (!name.length) name = [[NSProcessInfo processInfo] hostName];
        if (!name.length) return false;
        vp_copy_string(name, out, len);
        return true;
    }
}

bool vp_cpu_brand(char *out, size_t len) {
    if (!out || len == 0) return false;
    size_t size = len;
    if (sysctlbyname("machdep.cpu.brand_string", out, &size, NULL, 0) != 0) return false;
    out[len - 1] = 0;
    return out[0] != 0;
}

uint32_t vp_wake_acquire(void) {
    IOPMAssertionID assertion = 0;
    IOReturn result = IOPMAssertionCreateWithName(kIOPMAssertionTypePreventUserIdleDisplaySleep, kIOPMAssertionLevelOn,
                                                  CFSTR("Vision-Pro remote session"), &assertion);
    return result == kIOReturnSuccess ? assertion : 0;
}

void vp_wake_release(uint32_t assertion) {
    if (assertion) IOPMAssertionRelease(assertion);
}

/* ============================================================================================ */
/* Keychain                                                                                     */
/* ============================================================================================ */

static NSMutableDictionary *vp_keychain_query(const char *service, const char *account) {
    NSString *serviceName = service ? [NSString stringWithUTF8String:service] : nil;
    NSString *accountName = account ? [NSString stringWithUTF8String:account] : nil;
    if (!serviceName || !accountName) return nil;
    return [@{
        (__bridge id)kSecClass : (__bridge id)kSecClassGenericPassword,
        (__bridge id)kSecAttrService : serviceName,
        (__bridge id)kSecAttrAccount : accountName,
    } mutableCopy];
}

int32_t vp_keychain_get(const char *service, const char *account, uint8_t *out, size_t capacity, size_t *len) {
    @autoreleasepool {
        NSMutableDictionary *query = vp_keychain_query(service, account);
        if (!query || !len) return errSecParam;
        query[(__bridge id)kSecReturnData] = @YES;
        query[(__bridge id)kSecMatchLimit] = (__bridge id)kSecMatchLimitOne;
        CFTypeRef result = NULL;
        OSStatus status = SecItemCopyMatching((__bridge CFDictionaryRef)query, &result);
        if (status != errSecSuccess) return status;
        NSData *data = (__bridge_transfer NSData *)result;
        if (![data isKindOfClass:[NSData class]]) return errSecDecode;
        *len = data.length;
        if (!out || data.length > capacity) return errSecBufferTooSmall;
        memcpy(out, data.bytes, data.length);
        return errSecSuccess;
    }
}

int32_t vp_keychain_set(const char *service, const char *account, const uint8_t *data, size_t len) {
    @autoreleasepool {
        NSMutableDictionary *query = vp_keychain_query(service, account);
        if (!query || (!data && len)) return errSecParam;
        NSData *value = [NSData dataWithBytes:data length:len];
        OSStatus status = SecItemUpdate((__bridge CFDictionaryRef)query, (__bridge CFDictionaryRef) @{(__bridge id)kSecValueData : value});
        if (status == errSecItemNotFound) {
            query[(__bridge id)kSecValueData] = value;
            query[(__bridge id)kSecAttrLabel] = @"Vision-Pro";
            query[(__bridge id)kSecAttrDescription] = @"Vision-Pro agent secret";
            status = SecItemAdd((__bridge CFDictionaryRef)query, NULL);
        }
        return status;
    }
}

int32_t vp_keychain_delete(const char *service, const char *account) {
    @autoreleasepool {
        NSMutableDictionary *query = vp_keychain_query(service, account);
        if (!query) return errSecParam;
        return SecItemDelete((__bridge CFDictionaryRef)query);
    }
}
