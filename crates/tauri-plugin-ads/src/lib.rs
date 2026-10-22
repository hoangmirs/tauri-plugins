//! `tauri-plugin-ads`: shows Google Mobile Ads native ads over a slot the page
//! draws, and rewarded ads, on iOS and Android.
//!
//! No command ever returns an error. With no network, no ad or no native
//! side, a command answers `"failed"`, `"not_required"` or nothing. A desktop
//! shows no ads.

mod commands;
mod slot;

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

pub use slot::{Settings, Slot};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_ads);

/// One symbol from each of Google's objects, written by `build.rs`. See
/// `SKIP` there for why.
#[cfg(target_os = "ios")]
mod anchors {
    include!(concat!(env!("OUT_DIR"), "/anchors.rs"));
}

#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("ads")
        .invoke_handler(tauri::generate_handler![
            commands::init,
            commands::consent,
            commands::show_native,
            commands::hide_native,
            commands::load_rewarded,
            commands::show_rewarded,
            commands::privacy_options,
            commands::show_privacy_options,
            commands::set_muted
        ])
        .setup(|app, api| {
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle = api.register_android_plugin("dev.hoangmirs.ads", "AdsPlugin")?;
                #[cfg(target_os = "ios")]
                let handle = {
                    std::hint::black_box(anchors::ANCHORS.as_ptr());
                    api.register_ios_plugin(init_plugin_ads)?
                };
                app.manage(commands::Native(handle));
            }
            #[cfg(not(mobile))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}
