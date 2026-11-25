#[cfg(target_os = "macos")]
mod imp {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_foundation::{NSBundle, NSString};

    pub struct SparkleUpdater {
        controller: Option<Retained<AnyObject>>,
    }

    impl SparkleUpdater {
        pub fn init() -> Self {
            let Some(bundle_dir) = NSBundle::mainBundle().privateFrameworksPath() else {
                return Self { controller: None };
            };
            let sparkle_path = format!("{}/Sparkle.framework", bundle_dir);
            let ns_path = NSString::from_str(&sparkle_path);
            let Some(sparkle_bundle) = NSBundle::bundleWithPath(&ns_path) else {
                return Self { controller: None };
            };

            if !unsafe { sparkle_bundle.load() } {
                eprintln!("[hopr] Sparkle.framework not loaded (running outside .app bundle?)");
                return Self { controller: None };
            }

            let Some(cls) = AnyClass::get(c"SPUStandardUpdaterController") else {
                eprintln!("[hopr] SPUStandardUpdaterController class not found");
                return Self { controller: None };
            };

            unsafe {
                let alloc: *mut AnyObject = msg_send![cls, alloc];
                let nil: *mut AnyObject = std::ptr::null_mut();
                let ptr: *mut AnyObject = msg_send![
                    alloc,
                    initWithStartingUpdater: Bool::YES,
                    updaterDelegate: nil,
                    userDriverDelegate: nil
                ];
                let controller = Retained::retain(ptr);
                Self { controller }
            }
        }

        pub fn check_for_updates(&self) {
            let Some(ctrl) = &self.controller else {
                eprintln!("[hopr] Sparkle updater unavailable (run from Hopr.app bundle)");
                return;
            };
            unsafe {
                let nil: *mut AnyObject = std::ptr::null_mut();
                let _: () = msg_send![&**ctrl, checkForUpdates: nil];
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use crate::platform::open_external_url;

    pub struct SparkleUpdater;

    impl SparkleUpdater {
        pub fn init() -> Self {
            Self
        }

        pub fn check_for_updates(&self) {
            open_external_url("https://github.com/nowshadabir/hopr-releases/releases/latest");
        }
    }
}

pub use imp::SparkleUpdater;
