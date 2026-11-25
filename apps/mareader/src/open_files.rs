//! macOS: Finder's files, an Apple event winit leaves unhandled.
//! See docs/packaging.md section macOS.

#[cfg(all(target_os = "macos", feature = "open-files"))]
pub use mac::install;

/// Four-character Apple event codes, as AppKit spells them.
#[cfg(any(test, all(target_os = "macos", feature = "open-files")))]
pub const fn code(name: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*name)
}

/// Mac OS 9 launchers passed a process serial number as an argument.
pub fn is_launch_noise(arg: &std::ffi::OsStr) -> bool {
    arg.to_str().is_some_and(|a| a.starts_with("-psn_"))
}

#[cfg(all(target_os = "macos", feature = "open-files"))]
#[allow(unsafe_code)]
mod mac {
    use std::path::PathBuf;

    use objc2::rc::Retained;
    use objc2::runtime::NSObject;
    use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send, sel};
    use objc2_app_kit::NSApplicationWillFinishLaunchingNotification;
    use objc2_foundation::{
        NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter,
    };

    use super::code;
    use crate::inbox::Inbox;

    const CORE_EVENTS: u32 = code(b"aevt");
    const OPEN_DOCUMENTS: u32 = code(b"odoc");
    const DIRECT_OBJECT: u32 = code(b"----");

    pub struct Ivars {
        inbox: Inbox,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing rules; there is no Drop.
        #[unsafe(super(NSObject))]
        #[name = "MareaderOpenFiles"]
        #[ivars = Ivars]
        struct OpenFiles;

        impl OpenFiles {
            /// AppKit installs its own handlers just before this.
            #[unsafe(method(willFinishLaunching:))]
            fn will_finish_launching(&self, _note: &NSNotification) {
                let manager = NSAppleEventManager::sharedAppleEventManager();
                let handler = sel!(openDocuments:withReplyEvent:);
                // SAFETY: we answer `handler`, taking two descriptors.
                let () = unsafe {
                    msg_send![
                        &manager,
                        setEventHandler: self,
                        andSelector: handler,
                        forEventClass: CORE_EVENTS,
                        andEventID: OPEN_DOCUMENTS,
                    ]
                };
            }

            #[unsafe(method(openDocuments:withReplyEvent:))]
            fn open_documents(&self, event: &NSAppleEventDescriptor, _: &NSAppleEventDescriptor) {
                self.ivars().inbox.post(paths(event), false);
            }
        }
    );

    /// The file URLs an "open documents" event carries.
    fn paths(event: &NSAppleEventDescriptor) -> Vec<PathBuf> {
        // SAFETY: takes a four-character code; returns a descriptor or nil.
        let files: Option<Retained<NSAppleEventDescriptor>> =
            unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
        let Some(files) = files else {
            return Vec::new();
        };
        let path = |d: Retained<NSAppleEventDescriptor>| {
            Some(PathBuf::from(d.fileURLValue()?.path()?.to_string()))
        };
        // A list counts from one; a lone file is no list at all.
        match files.numberOfItems() {
            0 => path(files).into_iter().collect(),
            n => (1..=n)
                .filter_map(|i| path(files.descriptorAtIndex(i)?))
                .collect(),
        }
    }

    /// Call before eframe starts, while AppKit has yet to launch.
    pub fn install(inbox: Inbox) {
        let this = OpenFiles::alloc().set_ivars(Ivars { inbox });
        // SAFETY: NSObject's plain initialiser.
        let this: Retained<OpenFiles> = unsafe { msg_send![super(this), init] };
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: we answer the selector; the name is AppKit's constant.
        unsafe {
            center.addObserver_selector_name_object(
                &this,
                sel!(willFinishLaunching:),
                Some(NSApplicationWillFinishLaunchingNotification),
                None,
            );
        }
        // Neither the centre nor the event manager retains it.
        std::mem::forget(this);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_read_as_appkit_writes_them() {
        assert_eq!(code(b"aevt"), 0x6165_7674);
        assert_eq!(code(b"odoc"), 0x6F64_6F63);
    }

    #[test]
    fn launch_noise_is_not_a_file() {
        assert!(is_launch_noise("-psn_0_12345".as_ref()));
        assert!(!is_launch_noise("book.pdf".as_ref()));
    }
}
