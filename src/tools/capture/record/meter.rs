//! A live microphone level for the picker, so you can see the mic you chose
//! actually hears you before you record. macOS only; elsewhere it's silent.
// only the macOS meter uses this outside tests
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// dB (as AVFoundation reports power) → 0..1 for a meter: -60 dB and below
/// is empty, 0 dB full.
pub fn level_of(db: f32) -> f32 {
    if !db.is_finite() {
        return 0.0;
    }
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

#[cfg(target_os = "macos")]
pub use imp::MicMeter;

#[cfg(not(target_os = "macos"))]
pub struct MicMeter;

#[cfg(not(target_os = "macos"))]
impl MicMeter {
    pub fn open(_device: Option<&str>) -> Option<MicMeter> {
        None
    }
    pub fn level(&self) -> f32 {
        0.0
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use dispatch2::DispatchQueue;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{define_class, msg_send, AnyThread};
    use objc2_av_foundation::{AVCaptureAudioDataOutput, AVCaptureAudioDataOutputSampleBufferDelegate, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput, AVCaptureOutput, AVCaptureSession, AVMediaTypeAudio};
    use objc2_core_media::CMSampleBuffer;
    use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

    // Buffers have to be taken for the levels to move; nothing else to do.
    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "CmdMeowMicMeterSink"]
        struct Sink;

        unsafe impl NSObjectProtocol for Sink {}

        unsafe impl AVCaptureAudioDataOutputSampleBufferDelegate for Sink {
            #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
            fn did_output(&self, _output: &AVCaptureOutput, _sample: &CMSampleBuffer, _connection: &AVCaptureConnection) {}
        }
    );

    impl Sink {
        fn new() -> Retained<Self> {
            let this = Self::alloc().set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    pub struct MicMeter {
        session: Retained<AVCaptureSession>,
        output: Retained<AVCaptureAudioDataOutput>,
        _sink: Retained<Sink>,
    }

    struct SendBox<T>(T);
    unsafe impl<T> Send for SendBox<T> {}

    impl MicMeter {
        /// Listen to `device` (a unique id), or the default microphone.
        pub fn open(device: Option<&str>) -> Option<MicMeter> {
            unsafe {
                let media = AVMediaTypeAudio?;
                let dev = device.and_then(|id| AVCaptureDevice::deviceWithUniqueID(&NSString::from_str(id))).or_else(|| AVCaptureDevice::defaultDeviceWithMediaType(media))?;
                let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&dev).ok()?;
                let session = AVCaptureSession::new();
                let output = AVCaptureAudioDataOutput::new();
                if !session.canAddInput(&input) || !session.canAddOutput(&output) {
                    return None;
                }
                session.addInput(&input);
                session.addOutput(&output);
                let sink = Sink::new();
                let queue = DispatchQueue::new("cmdmeow.mic-meter", None);
                output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*sink)), Some(&queue));
                // starting takes a moment; not on the UI thread
                let s = SendBox(session.clone());
                std::thread::spawn(move || {
                    let s = s;
                    s.0.startRunning();
                });
                Some(MicMeter { session, output, _sink: sink })
            }
        }

        /// The loudest channel right now, 0..1.
        pub fn level(&self) -> f32 {
            unsafe {
                let mut db = f32::NEG_INFINITY;
                for c in self.output.connections().iter() {
                    for ch in c.audioChannels().iter() {
                        db = db.max(ch.averagePowerLevel());
                    }
                }
                super::level_of(db)
            }
        }
    }

    impl Drop for MicMeter {
        fn drop(&mut self) {
            let s = SendBox(self.session.clone());
            std::thread::spawn(move || {
                let s = s;
                unsafe { s.0.stopRunning() };
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_map_to_a_meter() {
        assert_eq!(level_of(-160.0), 0.0);
        assert_eq!(level_of(f32::NEG_INFINITY), 0.0);
        assert_eq!(level_of(0.0), 1.0);
        assert!((level_of(-30.0) - 0.5).abs() < 1e-6);
    }
}
