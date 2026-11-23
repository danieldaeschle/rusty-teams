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

#include "livekit/webrtc.h"

#include <algorithm>
#include <atomic>
#include <iostream>
#include <memory>

#include "livekit/audio_track.h"
#include "livekit/media_stream_track.h"
#include "livekit/rtp_receiver.h"
#include "livekit/rtp_sender.h"
#include "livekit/video_track.h"
#include "rtc_base/logging.h"
#include "rtc_base/crypto_random.h"
#include "rtc_base/synchronization/mutex.h"

#ifdef WEBRTC_WIN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <dshow.h>
#include <ks.h>
#include <ksmedia.h>
#include <wrl/client.h>

#include "rtc_base/win32.h"
#endif

namespace livekit_ffi {

static webrtc::Mutex g_mutex{};
// Can't be atomic, we're using a Mutex because we need to wait for the
// execution of the first init
static uint32_t g_release_counter(0);

RtcRuntime::RtcRuntime() {
  RTC_LOG(LS_VERBOSE) << "RtcRuntime()";

  {
    // Not the best way to do it...
    webrtc::MutexLock lock(&g_mutex);
    if (g_release_counter == 0) {
      RTC_CHECK(webrtc::InitializeSSL()) << "Failed to InitializeSSL()";

#ifdef WEBRTC_WIN
      WSADATA data;
      WSAStartup(MAKEWORD(1, 0), &data);
#endif
    }
    g_release_counter++;
  }

  network_thread_ = webrtc::Thread::CreateWithSocketServer();
  network_thread_->SetName("network_thread", &network_thread_);
  network_thread_->Start();
  worker_thread_ = webrtc::Thread::Create();
  worker_thread_->SetName("worker_thread", &worker_thread_);
  worker_thread_->Start();
  signaling_thread_ = webrtc::Thread::Create();
  signaling_thread_->SetName("signaling_thread", &signaling_thread_);
  signaling_thread_->Start();
}

RtcRuntime::~RtcRuntime() {
  RTC_LOG(LS_VERBOSE) << "~RtcRuntime()";

  worker_thread_->Stop();
  signaling_thread_->Stop();
  network_thread_->Stop();

  {
    webrtc::MutexLock lock(&g_mutex);
    g_release_counter--;
    if (g_release_counter == 0) {
      RTC_CHECK(webrtc::CleanupSSL()) << "Failed to CleanupSSL()";

#ifdef WEBRTC_WIN
      WSACleanup();
#endif
    }
  }
}

webrtc::Thread* RtcRuntime::network_thread() const {
  return network_thread_.get();
}

webrtc::Thread* RtcRuntime::worker_thread() const {
  return worker_thread_.get();
}

webrtc::Thread* RtcRuntime::signaling_thread() const {
  return signaling_thread_.get();
}

std::shared_ptr<MediaStreamTrack> RtcRuntime::get_or_create_media_stream_track(
    webrtc::scoped_refptr<webrtc::MediaStreamTrackInterface> rtc_track) {
  // A sender cleared with SetTrack(nullptr) has no track: the Rust side maps nullptr to None.
  if (!rtc_track) {
    return nullptr;
  }
  webrtc::MutexLock lock(&mutex_);
  for (std::weak_ptr<MediaStreamTrack> weak_existing_track :
       media_stream_tracks_) {
    if (std::shared_ptr<MediaStreamTrack> existing_track =
            weak_existing_track.lock()) {
      if (existing_track->rtc_track() == rtc_track) {
        return existing_track;
      }
    }
  }

  if (rtc_track->kind() == webrtc::MediaStreamTrackInterface::kVideoKind) {
    std::shared_ptr<VideoTrack> video_track =
        std::shared_ptr<VideoTrack>(new VideoTrack(
            shared_from_this(),
            webrtc::scoped_refptr<webrtc::VideoTrackInterface>(
                static_cast<webrtc::VideoTrackInterface*>(rtc_track.get()))));

    media_stream_tracks_.push_back(
        std::static_pointer_cast<MediaStreamTrack>(video_track));
    return video_track;
  } else {
    std::shared_ptr<AudioTrack> audio_track =
        std::shared_ptr<AudioTrack>(new AudioTrack(
            shared_from_this(),
            webrtc::scoped_refptr<webrtc::AudioTrackInterface>(
                static_cast<webrtc::AudioTrackInterface*>(rtc_track.get()))));

    media_stream_tracks_.push_back(
        std::static_pointer_cast<MediaStreamTrack>(audio_track));
    return audio_track;
  }
}

std::shared_ptr<AudioTrack> RtcRuntime::get_or_create_audio_track(
    webrtc::scoped_refptr<webrtc::AudioTrackInterface> track) {
  return std::static_pointer_cast<AudioTrack>(
      get_or_create_media_stream_track(track));
}

std::shared_ptr<VideoTrack> RtcRuntime::get_or_create_video_track(
    webrtc::scoped_refptr<webrtc::VideoTrackInterface> track) {
  return std::static_pointer_cast<VideoTrack>(
      get_or_create_media_stream_track(track));
}

LogSink::LogSink(
    rust::Fn<void(rust::String message, LoggingSeverity severity)> fnc)
    : fnc_(fnc) {
  // m150 replaced the old "release builds don't log" default with LoggingConfig,
  // which defaults to log_to_stderr=true / debug_severity=LS_INFO and installs
  // itself on the first log call. That writes straight to stderr, bypassing this
  // sink and therefore the Rust log facade, so nothing downstream can filter it.
  // Route everything through the sink only.
  webrtc::LogMessage::SetLogToStderr(false);
  webrtc::LogMessage::LogToDebug(webrtc::LoggingSeverity::LS_NONE);

  webrtc::LogMessage::AddLogToStream(this, webrtc::LoggingSeverity::LS_VERBOSE);
}

LogSink::~LogSink() {
  webrtc::LogMessage::RemoveLogToStream(this);
}

void LogSink::OnLogMessage(const std::string& message,
                           webrtc::LoggingSeverity severity) {
  fnc_(rust::String::lossy(message), static_cast<LoggingSeverity>(severity));
}

std::unique_ptr<LogSink> new_log_sink(
    rust::Fn<void(rust::String, LoggingSeverity)> fnc) {
  return std::make_unique<LogSink>(fnc);
}

rust::String create_random_uuid() {
  return webrtc::CreateRandomUuid();
}

void tune_windows_camera(rust::Str target) {
#ifdef WEBRTC_WIN
  if (target.empty()) {
    return;
  }
  using Microsoft::WRL::ComPtr;

  static const GUID kPropSetVidCapVideoProcAmp = {
      0xC6E13360,
      0x30AC,
      0x11d0,
      {0xA1, 0x8C, 0x00, 0xA0, 0xC9, 0x11, 0x89, 0x56}};
  static const GUID kPropSetVidCapCameraControl = {
      0xC6E13370,
      0x30AC,
      0x11d0,
      {0xA1, 0x8C, 0x00, 0xA0, 0xC9, 0x11, 0x89, 0x56}};

  std::wstring wanted;
  if (!target.empty()) {
    int wide_len = MultiByteToWideChar(CP_UTF8, 0, target.data(),
                                       static_cast<int>(target.size()), nullptr,
                                       0);
    if (wide_len > 0) {
      wanted.resize(wide_len);
      MultiByteToWideChar(CP_UTF8, 0, target.data(),
                          static_cast<int>(target.size()), wanted.data(),
                          wide_len);
    }
  }
  auto to_lower = [](std::wstring text) {
    for (wchar_t& ch : text) {
      if (ch >= L'A' && ch <= L'Z') {
        ch = static_cast<wchar_t>(ch - L'A' + L'a');
      }
    }
    return text;
  };
  const std::wstring wanted_lower = to_lower(wanted);
  if (wanted_lower.empty()) {
    return;
  }

  HRESULT hr_com = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
  const bool com_owned = SUCCEEDED(hr_com);

  ComPtr<ICreateDevEnum> dev_enum;
  if (SUCCEEDED(CoCreateInstance(CLSID_SystemDeviceEnum, nullptr,
                                 CLSCTX_INPROC_SERVER,
                                 IID_PPV_ARGS(&dev_enum))) &&
      dev_enum) {
    ComPtr<IEnumMoniker> enum_moniker;
    if (SUCCEEDED(dev_enum->CreateClassEnumerator(
            CLSID_VideoInputDeviceCategory, &enum_moniker, 0)) &&
        enum_moniker) {
      ComPtr<IMoniker> moniker;
      ULONG fetched = 0;
      while (enum_moniker->Next(1, &moniker, &fetched) == S_OK && moniker) {
        bool matches = false;
        if (!matches) {
          ComPtr<IPropertyBag> bag;
          if (SUCCEEDED(moniker->BindToStorage(nullptr, nullptr,
                                               IID_PPV_ARGS(&bag))) &&
              bag) {
            for (const wchar_t* prop : {L"DevicePath", L"FriendlyName"}) {
              VARIANT var;
              VariantInit(&var);
              if (SUCCEEDED(bag->Read(prop, &var, nullptr)) &&
                  var.vt == VT_BSTR && var.bstrVal) {
                std::wstring value_lower = to_lower(var.bstrVal);
                if (value_lower == wanted_lower ||
                    value_lower.find(wanted_lower) != std::wstring::npos ||
                    wanted_lower.find(value_lower) != std::wstring::npos) {
                  matches = true;
                }
              }
              VariantClear(&var);
              if (matches) {
                break;
              }
            }
          }
        }

        if (matches) {
          ComPtr<IBaseFilter> filter;
          if (SUCCEEDED(moniker->BindToObject(nullptr, nullptr,
                                              IID_PPV_ARGS(&filter))) &&
              filter) {
            ComPtr<IKsPropertySet> ks;
            if (SUCCEEDED(filter.As(&ks)) && ks) {
              // 50 Hz anti-flicker (power line frequency = 1)
              KSPROPERTY_VIDEOPROCAMP_S powerline = {};
              powerline.Property.Set = kPropSetVidCapVideoProcAmp;
              powerline.Property.Id =
                  KSPROPERTY_VIDEOPROCAMP_POWERLINE_FREQUENCY;
              powerline.Property.Flags = KSPROPERTY_TYPE_SET;
              powerline.Value = 1;
              powerline.Flags = KSPROPERTY_VIDEOPROCAMP_FLAGS_MANUAL;
              ks->Set(kPropSetVidCapVideoProcAmp,
                      KSPROPERTY_VIDEOPROCAMP_POWERLINE_FREQUENCY,
                      &powerline.Value,
                      sizeof(powerline) - sizeof(KSPROPERTY), &powerline,
                      sizeof(powerline));

              // Disable low-light auto-exposure frame-rate drop
              KSPROPERTY_CAMERACONTROL_S priority = {};
              priority.Property.Set = kPropSetVidCapCameraControl;
              priority.Property.Id =
                  KSPROPERTY_CAMERACONTROL_AUTO_EXPOSURE_PRIORITY;
              priority.Property.Flags = KSPROPERTY_TYPE_SET;
              priority.Value = 0;
              priority.Flags = KSPROPERTY_CAMERACONTROL_FLAGS_MANUAL;
              ks->Set(kPropSetVidCapCameraControl,
                      KSPROPERTY_CAMERACONTROL_AUTO_EXPOSURE_PRIORITY,
                      &priority.Value,
                      sizeof(priority) - sizeof(KSPROPERTY), &priority,
                      sizeof(priority));
            }
          }
          moniker.Reset();
          break;
        }
        moniker.Reset();
      }
    }
  }

  if (com_owned) {
    CoUninitialize();
  }
#else
  (void)target;
#endif
}

}  // namespace livekit_ffi
