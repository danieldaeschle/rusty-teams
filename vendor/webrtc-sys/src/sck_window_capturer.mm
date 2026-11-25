/*
 * Copyright 2025 LiveKit, Inc.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#include "livekit/sck_window_capturer.h"

#if defined(WEBRTC_MAC) && !defined(WEBRTC_IOS)

#import <CoreGraphics/CoreGraphics.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#import <Foundation/Foundation.h>
#import <IOSurface/IOSurface.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <dispatch/dispatch.h>

#include <climits>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <memory>
#include <utility>

#include "modules/desktop_capture/desktop_frame.h"
#include "modules/desktop_capture/desktop_geometry.h"
#include "modules/desktop_capture/mac/sck_picker_handle.h"

namespace {

constexpr size_t kMinCaptureDimension = 2;
constexpr size_t kMaxCaptureDimension = 16384;
constexpr uint32_t kDefaultMaxFps = 30;

livekit_ffi::SckCaptureStatus status_for_stop_error(NSError* error) {
  if ([error.domain isEqualToString:SCStreamErrorDomain]) {
    if (error.code == SCStreamErrorUserStopped) {
      return livekit_ffi::SckCaptureStatus::kStoppedByUser;
    }
    if (error.code == SCStreamErrorNoCaptureSource) {
      return livekit_ffi::SckCaptureStatus::kWindowGone;
    }
  }
  return livekit_ffi::SckCaptureStatus::kInterrupted;
}

size_t clamp_dimension(double pixels) {
  if (!std::isfinite(pixels) || pixels < static_cast<double>(kMinCaptureDimension)) {
    return kMinCaptureDimension;
  }
  if (pixels > static_cast<double>(kMaxCaptureDimension)) {
    return kMaxCaptureDimension;
  }
  return static_cast<size_t>(std::llround(pixels));
}

double clamp_double(double value, double lo, double hi) {
  if (value < lo) {
    return lo;
  }
  if (value > hi) {
    return hi;
  }
  return value;
}

}  // namespace

@interface LiveKitMacWindowSession
    : NSObject <SCStreamDelegate, SCStreamOutput, SCContentSharingPickerObserver>
- (instancetype)initWithMaxFps:(uint32_t)maxFps includeCursor:(BOOL)includeCursor;
- (void)startPickerWithPicker:(SCContentSharingPicker*)picker;
- (void)setMaxFps:(uint32_t)maxFps;
- (BOOL)pollFrame:(std::unique_ptr<webrtc::DesktopFrame>*)outFrame
           status:(livekit_ffi::SckCaptureStatus*)outStatus;
- (void)stop;
@end

@implementation LiveKitMacWindowSession {
  BOOL _stopped;
  BOOL _pickerCancelled;
  BOOL _pickerFailed;
  // Set when ScreenCaptureKit stops the running stream; kActive while running.
  livekit_ffi::SckCaptureStatus _endStatus;
  // The shared window is closed (macOS 15.2+). It may come back.
  BOOL _windowClosed;
  BOOL _includeCursor;
  uint32_t _maxFps;
  SCStream* _stream;
  SCContentFilter* _filter;
  size_t _configuredWidth;
  size_t _configuredHeight;
  IOSurfaceRef _latestSurface;
  CGRect _latestCropRect;
  dispatch_queue_t _captureQueue;
  // Guarded by @synchronized(self): read on the main queue when presenting and
  // cleared in stop.
  SCContentSharingPicker* _picker;
  SCContentSharingPickerConfiguration* _previousPickerConfig;
  BOOL _savedPickerConfig;
  // Serializes all updateContentFilter and updateConfiguration calls on _stream.
  // replayd checks each updateConfiguration against the picked content; one sent
  // before the picked filter is in place fails that check and falls back to the
  // Screen Recording permission. At most one async update runs at a time, and a
  // pending filter always runs before any configuration change (size or fps).
  BOOL _streamOpInFlight;
  BOOL _filterUpdateInFlight;
  SCContentFilter* _pendingFilter;
  BOOL _configDirty;
}

- (instancetype)initWithMaxFps:(uint32_t)maxFps includeCursor:(BOOL)includeCursor {
  self = [super init];
  if (self) {
    _endStatus = livekit_ffi::SckCaptureStatus::kActive;
    _maxFps = maxFps;
    _includeCursor = includeCursor;
    _captureQueue = dispatch_queue_create("org.rusty-teams.sck-window", DISPATCH_QUEUE_SERIAL);
  }
  return self;
}

- (void)dealloc {
  if (_latestSurface != NULL) {
    IOSurfaceDecrementUseCount(_latestSurface);
    CFRelease(_latestSurface);
    _latestSurface = NULL;
  }
}

- (SCStreamConfiguration*)buildConfigWithWidth:(size_t)width
                                        height:(size_t)height
                                        maxFps:(uint32_t)maxFps
                                 includeCursor:(BOOL)includeCursor {
  SCStreamConfiguration* config = [[SCStreamConfiguration alloc] init];
  config.pixelFormat = kCVPixelFormatType_32BGRA;
  config.colorSpaceName = kCGColorSpaceSRGB;
  config.showsCursor = includeCursor;
  config.captureResolution = SCCaptureResolutionBest;
  config.width = width < kMinCaptureDimension ? kMinCaptureDimension : width;
  config.height = height < kMinCaptureDimension ? kMinCaptureDimension : height;
  config.minimumFrameInterval =
      maxFps > 0 ? CMTimeMake(1, static_cast<int32_t>(maxFps)) : kCMTimeZero;
  return config;
}

// `picker` comes from a webrtc::SckPickerHandle, which owns its `active` and
// `maximumStreamCount` properties. They are shared with WebRTC's own SCK
// capturer, so this session must not change them.
- (void)startPickerWithPicker:(SCContentSharingPicker*)picker {
  @synchronized(self) {
    if (_stopped) {
      return;
    }
    _picker = picker;
    _previousPickerConfig = [picker.defaultConfiguration copy];
    _savedPickerConfig = YES;
  }
  [picker addObserver:self];
  SCContentSharingPickerConfiguration* config = [[SCContentSharingPickerConfiguration alloc] init];
  config.allowedPickerModes = SCContentSharingPickerModeSingleWindow;
  config.allowsChangingSelectedContent = YES;
  picker.defaultConfiguration = config;

  // ScreenCaptureKit presents the picker on the main queue. Async avoids
  // deadlocking if the main thread stops the capture and joins portal-share.
  __weak LiveKitMacWindowSession* weakSelf = self;
  dispatch_async(dispatch_get_main_queue(), ^{
    LiveKitMacWindowSession* strongSelf = weakSelf;
    if (strongSelf == nil) {
      return;
    }
    SCContentSharingPicker* activePicker = nil;
    @synchronized(strongSelf) {
      if (strongSelf->_stopped) {
        return;
      }
      activePicker = strongSelf->_picker;
    }
    if (activePicker != nil) {
      [activePicker presentPickerUsingContentStyle:SCShareableContentStyleWindow];
    }
  });
}

- (void)drainStreamUpdates {
  [self runNextStreamUpdateCompletingOp:NO wasFilter:NO forStream:nil filterError:nil];
}

// Transitions directly from a completing async SCStream op to the next queued
// update under one @synchronized(self) acquisition, so no resize frame can slip
// in between a picked filter update and its matching configuration update.
- (void)runNextStreamUpdateCompletingOp:(BOOL)completingOp
                              wasFilter:(BOOL)wasFilter
                              forStream:(nullable SCStream*)completedStream
                            filterError:(nullable NSError*)filterError {
  SCStream* stream = nil;
  SCContentFilter* filterToApply = nil;
  SCStreamConfiguration* configToApply = nil;

  @synchronized(self) {
    if (completingOp) {
      _streamOpInFlight = NO;
      if (wasFilter) {
        _filterUpdateInFlight = NO;
      }
      if (_stopped || _pickerFailed || completedStream == nil || _stream != completedStream) {
        return;
      }
      if (wasFilter && filterError != nil) {
        _pickerFailed = YES;
        _pendingFilter = nil;
        _configDirty = NO;
        return;
      }
    } else if (_stopped || _pickerFailed || _stream == nil || _streamOpInFlight) {
      return;
    }

    stream = _stream;
    if (_pendingFilter != nil) {
      filterToApply = _pendingFilter;
      _pendingFilter = nil;
      _streamOpInFlight = YES;
      _filterUpdateInFlight = YES;
    } else if (_configDirty && _configuredWidth >= kMinCaptureDimension &&
               _configuredHeight >= kMinCaptureDimension) {
      _configDirty = NO;
      configToApply = [self buildConfigWithWidth:_configuredWidth
                                          height:_configuredHeight
                                          maxFps:_maxFps
                                   includeCursor:_includeCursor];
      _streamOpInFlight = YES;
    } else {
      return;
    }
  }

  // Weak: the stream keeps this handler until the update completes.
  __weak LiveKitMacWindowSession* weakSelf = self;
  __weak SCStream* weakStream = stream;
  if (filterToApply != nil) {
    [stream updateContentFilter:filterToApply
              completionHandler:^(NSError* _Nullable error) {
                LiveKitMacWindowSession* strongSelf = weakSelf;
                if (strongSelf == nil) {
                  return;
                }
                [strongSelf runNextStreamUpdateCompletingOp:YES
                                                  wasFilter:YES
                                                  forStream:weakStream
                                                filterError:error];
              }];
    return;
  }

  [stream updateConfiguration:configToApply
            completionHandler:^(NSError* _Nullable configError) {
              (void)configError;
              LiveKitMacWindowSession* strongSelf = weakSelf;
              if (strongSelf == nil) {
                return;
              }
              [strongSelf runNextStreamUpdateCompletingOp:YES
                                                wasFilter:NO
                                                forStream:weakStream
                                              filterError:nil];
            }];
}

- (void)setMaxFps:(uint32_t)maxFps {
  BOOL shouldDrain = NO;
  @synchronized(self) {
    if (_stopped || _pickerFailed || _maxFps == maxFps) {
      return;
    }
    _maxFps = maxFps;
    if (_stream != nil && _configuredWidth >= kMinCaptureDimension &&
        _configuredHeight >= kMinCaptureDimension) {
      _configDirty = YES;
      shouldDrain = !_streamOpInFlight;
    }
  }
  if (shouldDrain) {
    [self drainStreamUpdates];
  }
}

- (BOOL)pollFrame:(std::unique_ptr<webrtc::DesktopFrame>*)outFrame
           status:(livekit_ffi::SckCaptureStatus*)outStatus {
  *outFrame = nullptr;
  *outStatus = livekit_ffi::SckCaptureStatus::kActive;

  IOSurfaceRef surface = NULL;
  CGRect crop = CGRectZero;
  @synchronized(self) {
    if (_stopped) {
      *outStatus = livekit_ffi::SckCaptureStatus::kPickerCancelled;
      return NO;
    }
    if (_pickerFailed) {
      *outStatus = livekit_ffi::SckCaptureStatus::kStartFailed;
      return NO;
    }
    if (_pickerCancelled) {
      *outStatus = livekit_ffi::SckCaptureStatus::kPickerCancelled;
      return NO;
    }
    if (_endStatus != livekit_ffi::SckCaptureStatus::kActive) {
      *outStatus = _endStatus;
      return NO;
    }
    if (_windowClosed) {
      *outStatus = livekit_ffi::SckCaptureStatus::kWindowGone;
      return NO;
    }
    if (_latestSurface == NULL) {
      *outStatus = livekit_ffi::SckCaptureStatus::kActive;
      return NO;
    }
    surface = _latestSurface;
    crop = _latestCropRect;
    CFRetain(surface);
    IOSurfaceIncrementUseCount(surface);
  }

  BOOL captured = NO;
  if (IOSurfaceLock(surface, kIOSurfaceLockReadOnly, NULL) == kIOReturnSuccess) {
    const uint8_t* base = static_cast<const uint8_t*>(IOSurfaceGetBaseAddress(surface));
    size_t surfW = IOSurfaceGetWidth(surface);
    size_t surfH = IOSurfaceGetHeight(surface);
    size_t stride = IOSurfaceGetBytesPerRow(surface);
    size_t bpp = IOSurfaceGetBytesPerElement(surface);
    if (base != NULL && bpp == 4 && surfW >= kMinCaptureDimension &&
        surfH >= kMinCaptureDimension && stride >= surfW * 4) {
      size_t x = 0;
      size_t y = 0;
      size_t w = surfW;
      size_t h = surfH;
      if (!CGRectIsEmpty(crop)) {
        size_t cx = static_cast<size_t>(crop.origin.x);
        size_t cy = static_cast<size_t>(crop.origin.y);
        size_t cw = static_cast<size_t>(crop.size.width);
        size_t ch = static_cast<size_t>(crop.size.height);
        if (cx < surfW && cy < surfH && cw >= kMinCaptureDimension && ch >= kMinCaptureDimension &&
            cx + cw <= surfW && cy + ch <= surfH) {
          x = cx;
          y = cy;
          w = cw;
          h = ch;
        }
      }
      size_t offset = y * stride + x * 4;
      size_t totalBytes = surfH * stride;
      if (offset < totalBytes) {
        size_t availableBytes = totalBytes - offset;
        size_t rowBytes = w * 4;
        size_t minNeeded = (h - 1) * stride + rowBytes;
        if (w <= INT_MAX && h <= INT_MAX && stride <= INT_MAX && availableBytes >= minNeeded) {
          auto frame = std::make_unique<webrtc::BasicDesktopFrame>(
              webrtc::DesktopSize(static_cast<int>(w), static_cast<int>(h)));
          frame->CopyPixelsFrom(
              base + offset,
              static_cast<int>(stride),
              webrtc::DesktopRect::MakeWH(static_cast<int>(w), static_cast<int>(h)));
          frame->set_may_contain_cursor(_includeCursor ? true : false);
          *outFrame = std::move(frame);
          captured = YES;
        }
      }
    }
    IOSurfaceUnlock(surface, kIOSurfaceLockReadOnly, NULL);
  }

  IOSurfaceDecrementUseCount(surface);
  CFRelease(surface);
  return captured;
}

- (void)stop {
  SCStream* streamToStop = nil;
  SCContentSharingPicker* picker = nil;
  SCContentSharingPickerConfiguration* previousConfig = nil;
  BOOL restoreConfig = NO;
  IOSurfaceRef surfaceToRelease = NULL;
  @synchronized(self) {
    if (_stopped) {
      return;
    }
    _stopped = YES;
    streamToStop = _stream;
    _stream = nil;
    _filter = nil;
    _pendingFilter = nil;
    _configDirty = NO;
    picker = _picker;
    _picker = nil;
    previousConfig = _previousPickerConfig;
    _previousPickerConfig = nil;
    restoreConfig = _savedPickerConfig;
    _savedPickerConfig = NO;
    surfaceToRelease = _latestSurface;
    _latestSurface = NULL;
  }

  if (picker != nil) {
    [picker removeObserver:self];
    if (restoreConfig) {
      picker.defaultConfiguration = previousConfig != nil ?
          previousConfig :
          [[SCContentSharingPickerConfiguration alloc] init];
    }
  }
  if (streamToStop != nil) {
    [streamToStop removeStreamOutput:self type:SCStreamOutputTypeScreen error:nil];
    [streamToStop stopCaptureWithCompletionHandler:nil];
  }

  if (surfaceToRelease != NULL) {
    IOSurfaceDecrementUseCount(surfaceToRelease);
    CFRelease(surfaceToRelease);
  }
}

- (void)maybeReconfigureWidth:(size_t)desiredW height:(size_t)desiredH {
  BOOL shouldDrain = NO;
  @synchronized(self) {
    // Do not overwrite dimensions or send a configuration update while a newly
    // picked filter is waiting or in flight; its completion applies the new
    // window's size and later frames resize from there.
    if (_stopped || _pickerFailed || _stream == nil || _pendingFilter != nil ||
        _filterUpdateInFlight) {
      return;
    }
    long dw = static_cast<long>(desiredW) - static_cast<long>(_configuredWidth);
    long dh = static_cast<long>(desiredH) - static_cast<long>(_configuredHeight);
    if (std::labs(dw) < 2 && std::labs(dh) < 2) {
      return;
    }
    _configuredWidth = desiredW;
    _configuredHeight = desiredH;
    _configDirty = YES;
    shouldDrain = !_streamOpInFlight;
  }
  if (shouldDrain) {
    [self drainStreamUpdates];
  }
}

- (void)contentSharingPicker:(SCContentSharingPicker*)picker
         didUpdateWithFilter:(SCContentFilter*)filter
                   forStream:(nullable SCStream*)stream {
  (void)picker;
  BOOL updateExisting = NO;
  SCStream* newStream = nil;
  SCStreamConfiguration* config = nil;
  dispatch_queue_t queue = nil;

  @synchronized(self) {
    if (_stopped || _pickerFailed) {
      return;
    }
    if (stream != nil && stream != _stream) {
      return;
    }

    CGRect contentRect = filter.contentRect;
    double scale = filter.pointPixelScale > 0.0f ? filter.pointPixelScale : 1.0;
    size_t width = clamp_dimension(contentRect.size.width * scale);
    size_t height = clamp_dimension(contentRect.size.height * scale);

    _filter = filter;
    _windowClosed = NO;
    _configuredWidth = width;
    _configuredHeight = height;

    if (_stream != nil) {
      _pendingFilter = filter;
      _configDirty = YES;
      updateExisting = YES;
    } else {
      config = [self buildConfigWithWidth:width
                                   height:height
                                   maxFps:_maxFps
                            includeCursor:_includeCursor];
      newStream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:self];
      _stream = newStream;
      queue = _captureQueue;
    }
  }

  if (updateExisting) {
    [self drainStreamUpdates];
    return;
  }

  if (newStream == nil) {
    @synchronized(self) {
      _pickerFailed = YES;
    }
    return;
  }

  // Rely on `picker.defaultConfiguration` rather than calling
  // `[picker setConfiguration:forStream:]`: mutating the per-stream picker
  // configuration dispatches an async `updateStream:withContentFilter:` IPC
  // whose filter no longer matches the picker-authorized filter in `replayd`,
  // causing `verifyContentPickerFilter:` to fail and trigger spurious Screen
  // Recording TCC prompts on macOS 27.

  NSError* error = nil;
  BOOL added = [newStream addStreamOutput:self
                                     type:SCStreamOutputTypeScreen
                       sampleHandlerQueue:queue
                                    error:&error];
  if (!added || error != nil) {
    @synchronized(self) {
      _pickerFailed = YES;
    }
    return;
  }

  __weak LiveKitMacWindowSession* weakSelf = self;
  [newStream startCaptureWithCompletionHandler:^(NSError* _Nullable startError) {
    if (startError == nil) {
      return;
    }
    LiveKitMacWindowSession* strongSelf = weakSelf;
    if (strongSelf == nil) {
      return;
    }
    // A capture that never starts is a failure to report, not a cancel.
    @synchronized(strongSelf) {
      strongSelf->_pickerFailed = YES;
    }
  }];
}

- (void)contentSharingPicker:(SCContentSharingPicker*)picker
          didCancelForStream:(nullable SCStream*)stream {
  (void)picker;
  @synchronized(self) {
    if (_stopped || _pickerFailed) {
      return;
    }
    // Only treat cancelling the initial picker as cancellation; cancelling a
    // mid-share "Change Window" sheet keeps the active capture running.
    if (stream == nil && _stream == nil) {
      _pickerCancelled = YES;
    }
  }
}

- (void)contentSharingPickerStartDidFailWithError:(NSError*)error {
  (void)error;
  @synchronized(self) {
    if (!_stopped && _stream == nil) {
      _pickerFailed = YES;
    }
  }
}

- (void)stream:(SCStream*)stream
    didOutputSampleBuffer:(CMSampleBufferRef)sampleBuffer
                   ofType:(SCStreamOutputType)type {
  (void)stream;
  if (type != SCStreamOutputTypeScreen) {
    return;
  }
  CVImageBufferRef imageBuffer = CMSampleBufferGetImageBuffer(sampleBuffer);
  if (imageBuffer == NULL) {
    return;
  }
  IOSurfaceRef ioSurface = CVPixelBufferGetIOSurface(imageBuffer);
  if (ioSurface == NULL) {
    return;
  }
  CFArrayRef attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, false);
  if (attachments == NULL || CFArrayGetCount(attachments) == 0) {
    return;
  }
  NSDictionary* dict = (__bridge NSDictionary*)CFArrayGetValueAtIndex(attachments, 0);
  if (dict == nil) {
    return;
  }

  NSNumber* statusNum = dict[SCStreamFrameInfoStatus];
  if (statusNum == nil) {
    return;
  }
  NSInteger status = [statusNum integerValue];
  if (status != SCFrameStatusComplete && status != SCFrameStatusStarted) {
    return;
  }

  double scaleFactor = 1.0;
  NSNumber* scaleNum = dict[SCStreamFrameInfoScaleFactor];
  if (scaleNum != nil && [scaleNum doubleValue] > 0.0) {
    scaleFactor = [scaleNum doubleValue];
  }

  double contentScale = 1.0;
  NSNumber* contentScaleNum = dict[SCStreamFrameInfoContentScale];
  if (contentScaleNum != nil && [contentScaleNum doubleValue] > 0.0) {
    contentScale = [contentScaleNum doubleValue];
  }

  CGRect contentRect = CGRectZero;
  id rectObj = dict[SCStreamFrameInfoContentRect];
  if (rectObj != nil) {
    if (!CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)rectObj, &contentRect)) {
      contentRect = CGRectZero;
    }
  }

  size_t surfW = IOSurfaceGetWidth(ioSurface);
  size_t surfH = IOSurfaceGetHeight(ioSurface);
  CGRect cropRect = CGRectZero;
  if (!CGRectIsEmpty(contentRect) && surfW >= kMinCaptureDimension &&
      surfH >= kMinCaptureDimension) {
    double rx = std::floor(contentRect.origin.x * scaleFactor);
    double ry = std::floor(contentRect.origin.y * scaleFactor);
    double rw = static_cast<double>(std::llround(contentRect.size.width * scaleFactor));
    double rh = static_cast<double>(std::llround(contentRect.size.height * scaleFactor));
    rx = clamp_double(rx, 0.0, static_cast<double>(surfW));
    ry = clamp_double(ry, 0.0, static_cast<double>(surfH));
    rw = clamp_double(rw, 0.0, static_cast<double>(surfW) - rx);
    rh = clamp_double(rh, 0.0, static_cast<double>(surfH) - ry);
    if (rw >= static_cast<double>(kMinCaptureDimension) &&
        rh >= static_cast<double>(kMinCaptureDimension)) {
      cropRect = CGRectMake(rx, ry, rw, rh);
    }
  }

  if (!CGRectIsEmpty(contentRect)) {
    size_t desiredW = clamp_dimension((contentRect.size.width / contentScale) * scaleFactor);
    size_t desiredH = clamp_dimension((contentRect.size.height / contentScale) * scaleFactor);
    [self maybeReconfigureWidth:desiredW height:desiredH];
  }

  IOSurfaceRef oldSurface = NULL;
  @synchronized(self) {
    if (_stopped || _pickerFailed) {
      return;
    }
    CFRetain(ioSurface);
    IOSurfaceIncrementUseCount(ioSurface);
    oldSurface = _latestSurface;
    _latestSurface = ioSurface;
    _latestCropRect = cropRect;
  }

  if (oldSurface != NULL) {
    IOSurfaceDecrementUseCount(oldSurface);
    CFRelease(oldSurface);
  }
}

// Stopping from the menu bar arrives as SCStreamErrorUserStopped.
- (void)stream:(SCStream*)stream didStopWithError:(NSError*)error {
  @synchronized(self) {
    if (!_stopped && stream == _stream && _endStatus == livekit_ffi::SckCaptureStatus::kActive) {
      _endStatus = status_for_stop_error(error);
    }
  }
}

// macOS 15.2+: every shared window was closed. The stream keeps running and
// would otherwise repeat the last frame.
- (void)streamDidBecomeInactive:(SCStream*)stream {
  @synchronized(self) {
    if (!_stopped && stream == _stream) {
      _windowClosed = YES;
    }
  }
}

- (void)streamDidBecomeActive:(SCStream*)stream {
  @synchronized(self) {
    if (stream == _stream) {
      _windowClosed = NO;
    }
  }
}

@end

namespace livekit_ffi {

namespace {

class SckWindowCapturerImpl final : public SckWindowCapturer {
 public:
  explicit SckWindowCapturerImpl(const webrtc::DesktopCaptureOptions& options)
      : include_cursor_(options.prefer_cursor_embedded()) {}

  ~SckWindowCapturerImpl() override {
    @autoreleasepool {
      if (session_ != nil) {
        [session_ stop];
        session_ = nil;
      }
      // After the session has stopped its stream and left the picker: the
      // last handle released deactivates the shared picker.
      picker_handle_.reset();
    }
  }

  void Start(Callback* callback) override {
    callback_ = callback;
    @autoreleasepool {
      if (session_ != nil) {
        [session_ stop];
        session_ = nil;
      }
      // Like WebRTC's ScreenCapturerSck, keep one handle for the capturer's
      // lifetime; each session runs at most one stream at a time.
      if (!picker_handle_) {
        picker_handle_ = webrtc::CreateSckPickerHandle();
      }
      if (!picker_handle_) {
        // The shared picker reached its stream limit. With no session,
        // CaptureFrame() reports kStartFailed.
        return;
      }
      session_ = [[LiveKitMacWindowSession alloc] initWithMaxFps:max_fps_
                                                   includeCursor:include_cursor_ ? YES : NO];
      [session_ startPickerWithPicker:picker_handle_->GetPicker()];
    }
  }

  void SetMaxFrameRate(uint32_t max_frame_rate) override {
    max_fps_ = max_frame_rate;
    @autoreleasepool {
      if (session_ != nil) {
        [session_ setMaxFps:max_fps_];
      }
    }
  }

  void CaptureFrame() override {
    if (!callback_) {
      return;
    }
    @autoreleasepool {
      if (session_ == nil) {
        last_status_ = SckCaptureStatus::kStartFailed;
        callback_->OnCaptureResult(Result::ERROR_PERMANENT, nullptr);
        return;
      }
      std::unique_ptr<webrtc::DesktopFrame> frame;
      SckCaptureStatus status = SckCaptureStatus::kActive;
      if ([session_ pollFrame:&frame status:&status] && frame) {
        last_status_ = SckCaptureStatus::kActive;
        callback_->OnCaptureResult(Result::SUCCESS, std::move(frame));
        return;
      }
      last_status_ = status;
      if (status == SckCaptureStatus::kActive || status == SckCaptureStatus::kWindowGone) {
        callback_->OnCaptureResult(Result::ERROR_TEMPORARY, nullptr);
      } else {
        callback_->OnCaptureResult(Result::ERROR_PERMANENT, nullptr);
      }
    }
  }

  bool GetSourceList(SourceList* sources) override {
    if (sources) {
      sources->clear();
    }
    return true;
  }

  bool SelectSource(SourceId /*id*/) override { return true; }

  SckCaptureStatus last_status() const override { return last_status_; }

 private:
  bool include_cursor_ = true;
  uint32_t max_fps_ = kDefaultMaxFps;
  Callback* callback_ = nullptr;
  SckCaptureStatus last_status_ = SckCaptureStatus::kActive;
  // Created and released on the capturer thread, as SckPickerHandle requires.
  std::unique_ptr<webrtc::SckPickerHandleInterface> picker_handle_;
  LiveKitMacWindowSession* session_ = nil;
};

}  // namespace

std::unique_ptr<SckWindowCapturer> CreateSckWindowCapturer(
    const webrtc::DesktopCaptureOptions& options) {
  return std::make_unique<SckWindowCapturerImpl>(options);
}

}  // namespace livekit_ffi

#endif  // defined(WEBRTC_MAC) && !defined(WEBRTC_IOS)
