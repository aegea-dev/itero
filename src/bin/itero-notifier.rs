//! itero-notifier: shows one desktop notification as itero. macOS gives a
//! notification its sender (name, icon, its own entry in System Settings,
//! Notifications) only when it comes from an app, so this runs inside
//! Itero.app (`make notifier`); `itero worker` starts it for notify@1.
//!
//! Usage: `itero-notifier <title> <text>`. Exits 0 once macOS has taken the
//! notification, 1 with the reason on stderr when it has not.

#[cfg(target_os = "macos")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [title, text] = args.as_slice() else {
        eprintln!("usage: itero-notifier <title> <text>");
        std::process::exit(2);
    };
    if let Err(e) = mac::show(title, text) {
        eprintln!("itero-notifier: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("itero-notifier is for macOS; elsewhere notify@1 runs notify-send");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
mod mac {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNUserNotificationCenter,
    };
    use std::sync::mpsc;
    use std::time::Duration;

    /// The first time, macOS asks the person whether itero may notify:
    /// they get this long to answer.
    const PROMPT: Duration = Duration::from_secs(120);

    /// An error a completion handler was given, as text.
    fn error(e: *mut NSError) -> Option<String> {
        // SAFETY: the handler's error is null or a valid NSError for the
        // handler's duration, and is only read here
        unsafe { e.as_ref() }.map(|e| e.localizedDescription().to_string())
    }

    pub fn show(title: &str, text: &str) -> Result<(), String> {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let (tx, rx) = mpsc::channel::<Result<bool, String>>();
        let answered = RcBlock::new(move |granted: Bool, e: *mut NSError| {
            let _ = tx.send(match error(e) {
                Some(why) => Err(why),
                None => Ok(granted.as_bool()),
            });
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &answered,
        );
        match rx.recv_timeout(PROMPT) {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => {
                return Err("notifications are off for itero: turn them on in System Settings, Notifications, itero".into());
            }
            Ok(Err(why)) => return Err(format!("cannot ask to notify: {why}")),
            Err(_) => return Err("no answer to the notifications prompt".into()),
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(text));
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let id = NSString::from_str(&format!("itero-{}-{nanos}", std::process::id()));
        let request =
            UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        let (tx, rx) = mpsc::channel::<Option<String>>();
        let added = RcBlock::new(move |e: *mut NSError| {
            let _ = tx.send(error(e));
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&added));
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(None) => Ok(()),
            Ok(Some(why)) => Err(format!("cannot show it: {why}")),
            Err(_) => Err("macOS did not take the notification".into()),
        }
    }
}
