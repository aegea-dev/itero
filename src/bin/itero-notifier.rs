//! itero-notifier: shows one desktop notification as itero. macOS gives a
//! notification its sender (name, icon, its own entry in System Settings,
//! Notifications) only when it comes from an app, so this runs inside
//! Itero.app (`make notifier`); `itero worker` starts it for notify@1.
//!
//! Usage: `itero-notifier <title> <text> [--subtitle S] [--sound S] [--url U]`.
//! Exits 0 once macOS has taken the notification, 1 with the reason on
//! stderr when it has not. A sound is `default` or a sound's name. With a
//! url (http or https), a click on the notification opens it: macOS starts
//! Itero.app again, with no arguments, to hand it the click.

#[cfg(target_os = "macos")]
const USAGE: &str = "usage: itero-notifier <title> <text> [--subtitle S] [--sound S] [--url U]";

#[cfg(target_os = "macos")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [title, text, rest @ ..] = args.as_slice() else {
        if args.is_empty() {
            mac::clicked();
            return;
        }
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let mut n = itero::engine::worker::Note {
        title,
        text,
        subtitle: None,
        sound: None,
        url: None,
    };
    for pair in rest.chunks(2) {
        let slot = match pair[0].as_str() {
            "--subtitle" => &mut n.subtitle,
            "--sound" => &mut n.sound,
            "--url" => &mut n.url,
            _ => {
                eprintln!("{USAGE}");
                std::process::exit(2);
            }
        };
        let Some(v) = pair.get(1) else {
            eprintln!("{USAGE}");
            std::process::exit(2);
        };
        *slot = Some(v);
    }
    if let Err(e) = mac::show(&n) {
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
    use itero::engine::worker::Note;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, ProtocolObject};
    use objc2::{ClassType, MainThreadMarker, define_class, msg_send};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSDictionary, NSError, NSObject, NSObjectProtocol, NSString, NSTimer};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNNotificationResponse, UNNotificationSound, UNUserNotificationCenter,
        UNUserNotificationCenterDelegate,
    };
    use std::sync::mpsc;
    use std::time::Duration;

    /// The first time, macOS asks the person whether itero may notify:
    /// they get this long to answer.
    const PROMPT: Duration = Duration::from_secs(120);
    /// Started for a click, how long it waits for macOS to hand it over.
    const CLICK: f64 = 10.0;
    /// The key of a notification's url in its userInfo.
    const URL: &str = "url";

    /// An error a completion handler was given, as text.
    fn error(e: *mut NSError) -> Option<String> {
        // SAFETY: the handler's error is null or a valid NSError for the
        // handler's duration, and is only read here
        unsafe { e.as_ref() }.map(|e| e.localizedDescription().to_string())
    }

    pub fn show(n: &Note) -> Result<(), String> {
        if let Some(u) = n.url
            && !itero::engine::worker::openable(u)
        {
            return Err(format!("a url to open is http or https: {u}"));
        }
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
        content.setTitle(&NSString::from_str(n.title));
        content.setBody(&NSString::from_str(n.text));
        if let Some(sub) = n.subtitle {
            content.setSubtitle(&NSString::from_str(sub));
        }
        // a name macOS does not find plays the default sound
        match n.sound {
            None => {}
            Some("default") => content.setSound(Some(&UNNotificationSound::defaultSound())),
            Some(name) => content.setSound(Some(&UNNotificationSound::soundNamed(
                &NSString::from_str(name),
            ))),
        }
        if let Some(u) = n.url {
            let info =
                NSDictionary::from_slices(&[&*NSString::from_str(URL)], &[&*NSString::from_str(u)]);
            // SAFETY: userInfo holds property-list values, as this does: a
            // string key and a string value, seen as any objects
            unsafe {
                let info: Retained<NSDictionary> = Retained::cast_unchecked(info);
                content.setUserInfo(&info);
            }
        }
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

    define_class!(
        /// Takes the click on a notification: opens its url, if it has one.
        #[unsafe(super(NSObject))]
        #[name = "IteroClicks"]
        struct Clicks;

        unsafe impl NSObjectProtocol for Clicks {}

        unsafe impl UNUserNotificationCenterDelegate for Clicks {
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                done: &block2::DynBlock<dyn Fn()>,
            ) {
                let info = response.notification().request().content().userInfo();
                let url = info
                    .objectForKey(&NSString::from_str(URL))
                    .and_then(|v| v.downcast::<NSString>().ok())
                    .map(|v| v.to_string());
                if let Some(u) = url.filter(|u| itero::engine::worker::openable(u))
                    && let Err(e) = std::process::Command::new("/usr/bin/open").arg(&u).status()
                {
                    eprintln!("itero-notifier: cannot open {u}: {e}");
                }
                done.call(());
                std::process::exit(0);
            }
        }
    );

    /// Started by a click: runs as an app until macOS hands the click over
    /// (`Clicks` ends it), or for `CLICK` seconds. macOS hands it only to
    /// an app that has finished launching, hence NSApplication; the
    /// delegate is set before, as it must be.
    pub fn clicked() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let center = UNUserNotificationCenter::currentNotificationCenter();
        // SAFETY: `new` on an NSObject subclass with no ivars of its own
        let clicks: Retained<Clicks> = unsafe { msg_send![Clicks::class(), new] };
        center.setDelegate(Some(ProtocolObject::from_ref(&*clicks)));
        let give_up = RcBlock::new(|_| std::process::exit(0));
        // SAFETY: the block only ends the process
        let _timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_repeats_block(CLICK, false, &give_up)
        };
        NSApplication::sharedApplication(mtm).run();
    }
}
