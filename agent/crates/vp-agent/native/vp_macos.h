/*
 * Vision-Pro macOS native bridge.
 *
 * A deliberately small C ABI over the Apple frameworks the agent needs:
 *   - ScreenCaptureKit  : GPU-resident display capture, scaled and converted to NV12 by the system
 *   - VideoToolbox      : hardware H.264 / HEVC encoding (Apple silicon media engine, Intel Quick Sync)
 *   - Quartz Event Services : mouse, keyboard, scroll, tablet (pressure / tilt) and magnify injection
 *   - Security (Keychain), IOKit power assertions, AppKit cursor and application metadata
 *
 * Every Objective-C object stays on this side of the boundary. Rust only sees opaque pointers,
 * plain structs and status codes, which keeps the unsafe surface small and auditable.
 */
#ifndef VP_MACOS_H
#define VP_MACOS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ------------------------------------------------------------------------------------------ */
/* Displays                                                                                   */
/* ------------------------------------------------------------------------------------------ */

typedef struct {
    uint32_t id;              /* CGDirectDisplayID */
    char name[128];           /* UTF-8, NUL terminated */
    double x, y;              /* global position in points (top-left origin of the main display) */
    double width, height;     /* size in points */
    uint32_t pixel_width;     /* backing pixels of the current mode */
    uint32_t pixel_height;
    uint32_t refresh_hz;
    bool primary;
    bool builtin;
} vp_display_t;

/* Fills up to `capacity` displays (mirrors are skipped). Returns the number written. */
int32_t vp_displays(vp_display_t *out, int32_t capacity);

/* ------------------------------------------------------------------------------------------ */
/* Privacy permissions (TCC)                                                                  */
/* ------------------------------------------------------------------------------------------ */

bool vp_screen_capture_allowed(void);
/* Shows the system prompt the first time; later calls only report the current state. */
bool vp_screen_capture_request(void);
bool vp_accessibility_allowed(bool prompt);
/* 0 = Screen Recording, 1 = Accessibility, 2 = Local Network. */
void vp_open_privacy_settings(int32_t pane);

/* ------------------------------------------------------------------------------------------ */
/* Capture (ScreenCaptureKit)                                                                 */
/* ------------------------------------------------------------------------------------------ */

typedef void *vp_capture_ref;
typedef void *vp_frame_ref; /* retained CVPixelBufferRef (NV12, BT.709 video range) */

vp_capture_ref vp_capture_start(uint32_t display_id, uint32_t width, uint32_t height, uint32_t fps,
                                char *error, size_t error_len);
/* 1 = a new frame was stored in *frame (caller releases it), 0 = timeout, -1 = stream stopped. */
int32_t vp_capture_next(vp_capture_ref capture, uint32_t timeout_ms, vp_frame_ref *frame);
/* Stops the stream and frees the capture. */
void vp_capture_stop(vp_capture_ref capture);

vp_frame_ref vp_frame_retain(vp_frame_ref frame);
void vp_frame_release(vp_frame_ref frame);
/* Copies the NV12 planes into tightly packed buffers (y: w*h, uv: w*h/2). */
bool vp_frame_copy_nv12(vp_frame_ref frame, uint8_t *y, uint8_t *uv, uint32_t width, uint32_t height);

/* ------------------------------------------------------------------------------------------ */
/* Encoding (VideoToolbox)                                                                    */
/* ------------------------------------------------------------------------------------------ */

typedef void *vp_encoder_ref;

#define VP_CODEC_H264 0
#define VP_CODEC_HEVC 1

/* Bit 0 = hardware H.264, bit 1 = hardware HEVC. */
uint32_t vp_hardware_encoders(void);

vp_encoder_ref vp_encoder_create(int32_t codec, uint32_t width, uint32_t height, uint32_t fps,
                                 uint32_t bitrate_kbps, bool prioritize_speed, char *error,
                                 size_t error_len);
void vp_encoder_set_bitrate(vp_encoder_ref encoder, uint32_t bitrate_kbps);
/* Encodes synchronously. On success *data points to an Annex B access unit that stays valid until
 * the next call (it may be empty when the encoder drops the frame). Returns an OSStatus. */
int32_t vp_encoder_encode(vp_encoder_ref encoder, vp_frame_ref frame, int64_t pts_us,
                          bool force_keyframe, const uint8_t **data, size_t *len, bool *keyframe);
bool vp_encoder_is_hardware(vp_encoder_ref encoder);
void vp_encoder_destroy(vp_encoder_ref encoder);

/* ------------------------------------------------------------------------------------------ */
/* Cursor                                                                                     */
/* ------------------------------------------------------------------------------------------ */

typedef struct {
    uint32_t width, height;
    int32_t hot_x, hot_y;
    uint64_t hash;
} vp_cursor_info_t;

/* Global cursor position in points. */
bool vp_cursor_location(double *x, double *y);
/* Renders the current system cursor as straight RGBA at `scale` pixels per point.
 * Returns 1 on success, 0 when no cursor is available, -1 when `capacity` is too small. */
int32_t vp_cursor_image(double scale, uint8_t *rgba, size_t capacity, vp_cursor_info_t *info);

/* ------------------------------------------------------------------------------------------ */
/* Input injection (Quartz Event Services)                                                    */
/* ------------------------------------------------------------------------------------------ */

#define VP_BUTTON_LEFT 0
#define VP_BUTTON_RIGHT 1
#define VP_BUTTON_MIDDLE 2
#define VP_BUTTON_BACK 3
#define VP_BUTTON_FORWARD 4

double vp_double_click_interval(void);
/* `held_button` is the lowest pressed button (-1 when none) so moves become drags. */
void vp_mouse_move(double x, double y, int32_t held_button, uint64_t flags);
void vp_mouse_button(double x, double y, int32_t button, bool down, int32_t click_count,
                     uint64_t flags);
/* Pixel-precise scroll (`lines` = false) or line scroll. Positive = toward the top / left. */
void vp_scroll(int32_t vertical, int32_t horizontal, bool lines, uint64_t flags);
void vp_key(uint16_t keycode, bool down, uint64_t flags, bool modifier);
void vp_text(const uint16_t *units, size_t count);
/* NX_KEYTYPE_* media key (sound up/down, mute, play, next, previous). */
void vp_media_key(int32_t key, bool down);

/* Tablet pen: proximity and points with pressure (0..1), tilt (-1..1) and rotation (degrees). */
void vp_tablet_proximity(bool entering, bool eraser);
/* phase: 0 hover, 1 down, 2 drag, 3 up. */
void vp_tablet_point(double x, double y, int32_t phase, int32_t button, double pressure,
                     double tilt_x, double tilt_y, double rotation, uint64_t flags);

/* Trackpad magnify gesture. phase: 0 begin, 1 change, 2 end. */
void vp_magnify(int32_t phase, double magnification, double x, double y);

/* ------------------------------------------------------------------------------------------ */
/* System                                                                                     */
/* ------------------------------------------------------------------------------------------ */

/* Bundle identifier and localized name of the application owning the frontmost window. */
bool vp_frontmost_app(char *bundle_id, size_t bundle_len, char *name, size_t name_len);
/* "Bryan's MacBook Pro" as set in System Settings → General → About. */
bool vp_computer_name(char *out, size_t len);
/* "Apple M3 Pro", "Intel(R) Core(TM) i9-9980HK …". */
bool vp_cpu_brand(char *out, size_t len);

/* Prevents display and idle system sleep. Returns 0 on failure. */
uint32_t vp_wake_acquire(void);
void vp_wake_release(uint32_t assertion);

/* ------------------------------------------------------------------------------------------ */
/* Keychain (login keychain, generic password)                                                */
/* ------------------------------------------------------------------------------------------ */

/* Returns an OSStatus (0 = success, -25300 = not found). */
int32_t vp_keychain_get(const char *service, const char *account, uint8_t *out, size_t capacity,
                        size_t *len);
int32_t vp_keychain_set(const char *service, const char *account, const uint8_t *data, size_t len);
int32_t vp_keychain_delete(const char *service, const char *account);

#ifdef __cplusplus
}
#endif

#endif
