//! itero-notifier: shows one desktop notification as itero. macOS gives a
//! notification its sender (name, icon, its own entry in System Settings,
//! Notifications) only when it comes from an app, so this runs inside
//! Itero.app (`make notifier`); `itero worker` starts it for notify@1.
//!
//! `itero-notifier --status` prints macOS's setting for Itero's
//! notifications (allowed, off, not asked, provisional), showing nothing.
//!
//! `--result <file>` (any notification) writes `ok`, or the error, to that
//! file as it ends: for a launch through LaunchServices, whose output and
//! exit status the launcher does not get.
//!
//! Usage: `itero-notifier <title> <text> [--<name> <value>]...`, a name of
//! `Note::fields`: subtitle, sound, url, buttons (a JSON list of labels),
//! wait, and what a click is told with (notification, workspace, itero,
//! workflow, data). Exits 0 once macOS has taken the notification, 1 with
//! the reason on stderr when it has not.
//!
//! A click starts Itero.app again, with no arguments, to hand it over: it
//! opens the url on a click on the notification itself, and tells the
//! workspace that showed it with `itero notification-clicked`.

#[cfg(target_os = "macos")]
const USAGE: &str = "usage: itero-notifier <title> <text> [--<name> <value>]...";

#[cfg(target_os = "macos")]
fn usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    use itero::engine::worker::Note;
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--status"] {
        match mac::status() {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("itero-notifier: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.is_empty() {
        mac::clicked();
        return;
    }
    let Some((n, result)) = Note::from_args(&args) else {
        usage()
    };
    let shown = mac::show(&n);
    if let Some(path) = result {
        // whole or not at all: written beside, then renamed
        let tmp = format!("{path}.tmp");
        let said = match &shown {
            Ok(()) => "ok".to_string(),
            Err(e) => e.clone(),
        };
        if std::fs::write(&tmp, said).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
    if let Err(e) = shown {
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
    use objc2_foundation::{
        NSArray, NSDictionary, NSError, NSObject, NSObjectProtocol, NSSet, NSString, NSTimer,
    };
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationAction,
        UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
        UNNotificationDefaultActionIdentifier, UNNotificationDismissActionIdentifier,
        UNNotificationRequest, UNNotificationResponse, UNNotificationSound,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    /// The first time, macOS asks the person whether itero may notify:
    /// they get this long to answer.
    const PROMPT: Duration = Duration::from_secs(120);
    /// Started for a click, how long it waits for macOS to hand it over.
    const CLICK: f64 = 10.0;
    /// What a notification keeps in its userInfo, for a click on it.
    const KEPT: [&str; 6] = [
        "url",
        "notification",
        "workspace",
        "itero",
        "workflow",
        "data",
    ];

    /// An error a completion handler was given, as text.
    fn error(e: *mut NSError) -> Option<String> {
        // SAFETY: the handler's error is null or a valid NSError for the
        // handler's duration, and is only read here
        unsafe { e.as_ref() }.map(|e| e.localizedDescription().to_string())
    }

    /// The app's categories, moved from the handler's thread to this one.
    struct Categories(Vec<Retained<UNNotificationCategory>>);
    // SAFETY: categories are immutable, and their reference counts are
    // atomic: they are only moved across, and read on the receiving side
    unsafe impl Send for Categories {}

    /// The category for a notification's buttons, added to the app's
    /// others (setting them replaces them all, and one still shown keeps
    /// its own buttons); one that waits is told of a dismissal too.
    fn category(center: &UNUserNotificationCenter, n: &Note) -> Option<Retained<NSString>> {
        if n.buttons.is_empty() && !n.wait {
            return None;
        }
        let labels = serde_json::to_string(&n.buttons).unwrap_or_default();
        let id = NSString::from_str(&format!(
            "itero{}{labels}",
            if n.wait { "+wait" } else { "" }
        ));
        let actions: Vec<_> = n
            .buttons
            .iter()
            .map(|b| {
                let b = NSString::from_str(b);
                UNNotificationAction::actionWithIdentifier_title_options(
                    &b,
                    &b,
                    UNNotificationActionOptions(0),
                )
            })
            .collect();
        let options = if n.wait {
            UNNotificationCategoryOptions::CustomDismissAction
        } else {
            UNNotificationCategoryOptions::empty()
        };
        let this = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
            &id,
            &NSArray::from_retained_slice(&actions),
            &NSArray::new(),
            options,
        );
        let (tx, rx) = mpsc::channel::<Categories>();
        let got = RcBlock::new(move |set: NonNull<NSSet<UNNotificationCategory>>| {
            // SAFETY: the handler's set is valid for its duration
            let _ = tx.send(Categories(unsafe { set.as_ref() }.to_vec()));
        });
        center.getNotificationCategoriesWithCompletionHandler(&got);
        let mut all = rx
            .recv_timeout(Duration::from_secs(10))
            .map(|c| c.0)
            .unwrap_or_default();
        all.retain(|c| c.identifier().to_string() != id.to_string());
        all.push(this);
        center.setNotificationCategories(&NSSet::from_retained_slice(&all));
        Some(id)
    }

    /// macOS's setting for these notifications, in words; nothing shown.
    pub fn status() -> Result<&'static str, String> {
        use objc2_user_notifications::{UNAuthorizationStatus, UNNotificationSettings};
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let (tx, rx) = mpsc::channel::<UNAuthorizationStatus>();
        let answered = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            // SAFETY: macOS hands the block settings that live through it
            let _ = tx.send(unsafe { settings.as_ref() }.authorizationStatus());
        });
        center.getNotificationSettingsWithCompletionHandler(&answered);
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(UNAuthorizationStatus::Authorized) => Ok("allowed"),
            Ok(UNAuthorizationStatus::Denied) => Ok("off"),
            Ok(UNAuthorizationStatus::NotDetermined) => Ok("not asked"),
            Ok(UNAuthorizationStatus::Provisional) => Ok("provisional"),
            Ok(_) => Ok("allowed for a while"),
            Err(_) => Err("no answer from macOS's notification settings".into()),
        }
    }

    pub fn show(n: &Note) -> Result<(), String> {
        if let Some(u) = &n.url
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
                return Err("notifications are off for Itero: turn them on in System Settings, Notifications, Itero".into());
            }
            Ok(Err(why)) => return Err(format!("cannot ask to notify: {why}")),
            Err(_) => return Err("no answer to the notifications prompt".into()),
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&n.title));
        content.setBody(&NSString::from_str(&n.text));
        if let Some(sub) = &n.subtitle {
            content.setSubtitle(&NSString::from_str(sub));
        }
        // a name macOS does not find plays the default sound
        match n.sound.as_deref() {
            None => {}
            Some("default") => content.setSound(Some(&UNNotificationSound::defaultSound())),
            Some(name) => content.setSound(Some(&UNNotificationSound::soundNamed(
                &NSString::from_str(name),
            ))),
        }
        let kept: Vec<(Retained<NSString>, Retained<NSString>)> = n
            .fields(true)
            .into_iter()
            .filter(|(k, _)| KEPT.contains(k))
            .map(|(k, v)| (NSString::from_str(k), NSString::from_str(&v)))
            .collect();
        if !kept.is_empty() {
            let keys: Vec<&NSString> = kept.iter().map(|(k, _)| &**k).collect();
            let values: Vec<&NSString> = kept.iter().map(|(_, v)| &**v).collect();
            let info = NSDictionary::from_slices(&keys, &values);
            // SAFETY: userInfo holds property-list values, as this does:
            // string keys and string values, seen as any objects
            unsafe {
                let info: Retained<NSDictionary> = Retained::cast_unchecked(info);
                content.setUserInfo(&info);
            }
        }
        if let Some(id) = category(&center, n) {
            content.setCategoryIdentifier(&id);
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

    /// A click, as the response says: `open` on the notification itself,
    /// `dismissed` when closed without one, else the button's label.
    fn answer(response: &UNNotificationResponse) -> String {
        let action = response.actionIdentifier();
        // SAFETY: the two are constant strings UserNotifications exports
        let (open, dismissed) = unsafe {
            (
                UNNotificationDefaultActionIdentifier,
                UNNotificationDismissActionIdentifier,
            )
        };
        if *action == *open {
            "open".into()
        } else if *action == *dismissed {
            "dismissed".into()
        } else {
            action.to_string()
        }
    }

    define_class!(
        /// Takes the click on a notification: opens its url on a click on
        /// it, and tells the workspace that showed it.
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
                let get = |k: &str| {
                    info.objectForKey(&NSString::from_str(k))
                        .and_then(|v| v.downcast::<NSString>().ok())
                        .map(|v| v.to_string())
                };
                let answer = answer(response);
                if answer == "open"
                    && let Some(u) = get("url").filter(|u| itero::engine::worker::openable(u))
                    && let Err(e) = std::process::Command::new("/usr/bin/open").arg(&u).status()
                {
                    eprintln!("itero-notifier: cannot open {u}: {e}");
                }
                // an older itero's notification has no workspace: opened only
                if let (Some(id), Some(workspace), Some(itero)) =
                    (get("notification"), get("workspace"), get("itero"))
                {
                    let mut click = serde_json::json!({"notification": id, "answer": answer});
                    if let Some(w) = get("workflow") {
                        click["workflow"] = w.into();
                    }
                    if let Some(d) = get("data").and_then(|d| serde_json::from_str(&d).ok()) {
                        click["data"] = d;
                    }
                    let told = std::process::Command::new(&itero)
                        .arg("notification-clicked")
                        .arg(click.to_string())
                        .current_dir(&workspace)
                        .status();
                    if !told.as_ref().is_ok_and(|s| s.success()) {
                        eprintln!("itero-notifier: cannot tell {workspace} of the click: {told:?}");
                    }
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
