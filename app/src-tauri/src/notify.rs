//! The daily notification, handed to the Mac's own notification centre
//! (UNUserNotificationCenter), which delivers it at its time whether or not
//! the window is open. `mg_core::reminder` decides what to schedule; this
//! asks the learner's permission, replaces what is pending, and opens the
//! window on the notification's workspace when the learner clicks it.
//!
//! The Mac lets only an app bundle use it: the window run as a bare program
//! (in development, in the harness) has no notifications, and says so.

use std::path::Path;
use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use mg_core::reminder::{self, Kept, Notice};

/// Set to ask the Mac for quiet (provisional) permission, which it grants
/// with no prompt: for a test build, since the real prompt is the learner's
/// to answer and no check may answer it.
pub const PROVISIONAL_ENV: &str = "MAGNESIUM_APP_NOTIFY_PROVISIONAL";

/// What the Mac allows this app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// The learner has not been asked.
    NotAsked,
    Allowed,
    /// Delivered quietly to Notification Center only, with no banner.
    Quiet,
    /// The learner refused, or turned Magnesium's notifications off in System Settings.
    Denied,
    /// This is not an app bundle, so the Mac has no notifications for it.
    Unavailable,
}

/// The daily notification as the page shows it in Settings.
#[derive(Clone, Debug, Serialize)]
pub struct State {
    pub on: bool,
    /// "HH:MM", the time on a day with no planned session.
    pub time: String,
    pub permission: Permission,
    /// The next one scheduled: when (ms since the epoch) and its words.
    pub next: Option<Next>,
    /// One an earlier build of the app handed over that this one cannot
    /// reach: it still comes, on or off.
    pub held: Option<Next>,
    /// Why the window turned the notification off, when it did.
    pub problem: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Next {
    pub at_ms: i64,
    pub title: String,
    pub body: String,
}

fn next_of(notice: &Notice) -> Option<Next> {
    Some(Next {
        at_ms: reminder::ms_of(notice.at)?,
        title: notice.title.clone(),
        body: notice.body.clone(),
    })
}

fn state(kept: &Kept, notices: &[Notice], permission: Permission) -> State {
    State {
        on: kept.on,
        time: kept.time().format("%H:%M").to_string(),
        permission,
        next: notices.first().and_then(next_of),
        held: kept.held.first().and_then(next_of),
        problem: kept.problem.clone(),
    }
}

/// One schedule at a time: the window, and the program the runtime starts
/// after a marked answer, may both be at it at once. Without this, a schedule
/// worked out from a setting read just before the learner turned it off
/// could be written back over the "off".
fn locked<T>(root: &Path, work: impl FnOnce() -> T) -> T {
    let _ = std::fs::create_dir_all(root);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join("notification.lock"));
    if let Ok(file) = &file {
        let _ = file.lock();
    }
    let done = work();
    if let Ok(file) = &file {
        let _ = file.unlock();
    }
    done
}

/// Work out the schedule from the record and hand it to the Mac. Returns
/// what the page shows.
pub fn refresh(root: &Path, folders: &[String]) -> State {
    locked(root, || refresh_now(root, folders))
}

/// As `refresh`, for the program the runtime starts after each recorded
/// answer: a run that finds another at it leaves it a mark and ends, so
/// runs never queue.
pub fn refresh_once(root: &Path, folders: &[String]) -> Kept {
    reminder::coalesce(root, || {
        refresh_now(root, folders);
    });
    Kept::read(root)
}

/// Turn the notification on or off, or set its time, then schedule again.
/// `problem` says why the window turned it off, when it did.
pub fn change(
    root: &Path,
    folders: &[String],
    on: bool,
    time: Option<String>,
    problem: Option<String>,
) -> State {
    locked(root, || {
        let mut kept = Kept::read(root);
        kept.on = on;
        kept.problem = problem;
        if let Some(time) = time {
            kept.time = Some(time);
        }
        kept.write(root);
        refresh_now(root, folders)
    })
}

fn refresh_now(root: &Path, folders: &[String]) -> State {
    let permission = permission();
    // A window that is not an app bundle hands the Mac nothing, so it notes
    // nothing as scheduled: the installed app sharing its folder would
    // otherwise take those days as held by another build.
    if permission == Permission::Unavailable {
        return state(&Kept::read(root), &[], permission);
    }
    let pending = pending();
    let (kept, notices) = reminder::plan(root, folders, &build(), pending.as_deref());
    let handed = match permission {
        Permission::Allowed | Permission::Quiet => &notices[..],
        // Nothing can arrive; anything still pending from before is taken away.
        _ => &[],
    };
    replace(handed);
    state(&kept, &notices, permission)
}

/// The notification as it stands, without scheduling anything.
pub fn current(root: &Path) -> State {
    let kept = Kept::read(root);
    let now = reminder::now();
    let ahead: Vec<Notice> = kept
        .scheduled
        .iter()
        .filter(|n| n.at > now)
        .cloned()
        .collect();
    state(&kept, &ahead, permission())
}

/// This build of the app, as the Mac tells builds apart: by the program
/// itself. The same program, started by the runtime or as the window, is
/// the same build.
fn build() -> String {
    std::env::current_exe()
        .and_then(std::fs::metadata)
        .map(|meta| {
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_secs());
            format!("{}-{modified}", meta.len())
        })
        .unwrap_or_default()
}

static APP: OnceLock<AppHandle> = OnceLock::new();

/// A notification was clicked: open the window on its workspace. The page
/// may not be listening yet when a click starts the app, so the folder is
/// also kept for the page to ask for (`take_opened`).
fn opened(folder: String) {
    // A workspace gone since the notification was scheduled: the window only
    // comes to the front, on whatever it shows.
    let there = Path::new(&folder).is_dir();
    if there {
        *lock_opened() = Some(folder.clone());
    }
    if let Some(app) = APP.get() {
        if there {
            let _ = app.emit(
                "menu",
                serde_json::json!({ "command": "go", "arg": folder }),
            );
        }
        if let Some(window) = app.webview_windows().values().next() {
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }
}

fn lock_opened() -> std::sync::MutexGuard<'static, Option<String>> {
    static OPENED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    OPENED.lock().unwrap_or_else(|p| p.into_inner())
}

/// The workspace a clicked notification asked for, once.
pub fn take_opened() -> Option<String> {
    lock_opened().take()
}

/// For the harness: what a click on a notification for `folder` does.
pub fn open_as_if_clicked(folder: String) {
    opened(folder);
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    use block2::RcBlock;
    use chrono::{Datelike, Timelike};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject, ProtocolObject};
    use objc2::{define_class, msg_send, AnyThread};
    use objc2_foundation::{
        NSArray, NSBundle, NSDateComponents, NSDictionary, NSError, NSObjectProtocol, NSString,
    };
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNCalendarNotificationTrigger,
        UNMutableNotificationContent, UNNotificationRequest, UNNotificationResponse,
        UNNotificationSettings, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    use super::Permission;
    use mg_core::reminder::{Notice, ID_PREFIX};

    const WAIT: Duration = Duration::from_secs(10);
    const FOLDER_KEY: &str = "folder";

    pub fn bundle_id() -> Option<String> {
        NSBundle::mainBundle()
            .bundleIdentifier()
            .map(|id| id.to_string())
    }

    /// Only an app bundle has a notification centre; asking for one outside
    /// a bundle stops the program.
    pub fn packaged() -> bool {
        let bundle = NSBundle::mainBundle();
        bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
    }

    fn center() -> Option<Retained<UNUserNotificationCenter>> {
        packaged().then(UNUserNotificationCenter::currentNotificationCenter)
    }

    fn of(status: UNAuthorizationStatus) -> Permission {
        match status {
            UNAuthorizationStatus::Authorized | UNAuthorizationStatus::Ephemeral => {
                Permission::Allowed
            }
            UNAuthorizationStatus::Provisional => Permission::Quiet,
            UNAuthorizationStatus::Denied => Permission::Denied,
            _ => Permission::NotAsked,
        }
    }

    pub fn permission() -> Permission {
        let Some(center) = center() else {
            return Permission::Unavailable;
        };
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            let _ = tx.send(of(unsafe { settings.as_ref() }.authorizationStatus()));
        });
        center.getNotificationSettingsWithCompletionHandler(&block);
        rx.recv_timeout(WAIT).unwrap_or(Permission::NotAsked)
    }

    /// Ask for permission. The first time, the Mac shows the learner its
    /// own prompt and this waits for their answer.
    pub fn ask(provisional: bool) -> Permission {
        let Some(center) = center() else {
            return Permission::Unavailable;
        };
        let options = if provisional {
            UNAuthorizationOptions::Provisional | UNAuthorizationOptions::Alert
        } else {
            UNAuthorizationOptions::Alert
        };
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |_granted: Bool, _error: *mut NSError| {
            let _ = tx.send(());
        });
        center.requestAuthorizationWithOptions_completionHandler(options, &block);
        // The learner may take their time over the Mac's prompt.
        let _ = rx.recv_timeout(Duration::from_secs(600));
        permission()
    }

    /// The identifiers this build finds waiting with the Mac; None outside
    /// an app bundle.
    pub fn pending() -> Option<Vec<String>> {
        center().map(|center| pending_ids(&center))
    }

    fn pending_ids(center: &UNUserNotificationCenter) -> Vec<String> {
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |list: NonNull<NSArray<UNNotificationRequest>>| {
            let ids = unsafe { list.as_ref() }
                .iter()
                .map(|request| request.identifier().to_string())
                .collect::<Vec<_>>();
            let _ = tx.send(ids);
        });
        center.getPendingNotificationRequestsWithCompletionHandler(&block);
        rx.recv_timeout(WAIT).unwrap_or_default()
    }

    /// Take away every daily notification still pending, and schedule these.
    pub fn replace(notices: &[Notice]) {
        let Some(center) = center() else {
            return;
        };
        let old = pending_ids(&center)
            .into_iter()
            .filter(|id| id.starts_with(ID_PREFIX))
            .map(|id| NSString::from_str(&id))
            .collect::<Vec<_>>();
        if !old.is_empty() {
            let refs = old.iter().map(|id| &**id).collect::<Vec<_>>();
            center.removePendingNotificationRequestsWithIdentifiers(&NSArray::from_slice(&refs));
        }
        for notice in notices {
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&notice.title));
            content.setBody(&NSString::from_str(&notice.body));
            let key = NSString::from_str(FOLDER_KEY);
            let folder = NSString::from_str(&notice.folder);
            let info = NSDictionary::<NSString, NSString>::from_slices(&[&*key], &[&*folder]);
            // SAFETY: a dictionary of strings is a property list, as userInfo needs.
            unsafe {
                let info: &NSDictionary =
                    &*(&*info as *const NSDictionary<NSString, NSString> as *const NSDictionary);
                content.setUserInfo(info);
            }
            let when = NSDateComponents::new();
            when.setYear(notice.at.year() as isize);
            when.setMonth(notice.at.month() as isize);
            when.setDay(notice.at.day() as isize);
            when.setHour(notice.at.hour() as isize);
            when.setMinute(notice.at.minute() as isize);
            let trigger = UNCalendarNotificationTrigger::triggerWithDateMatchingComponents_repeats(
                &when, false,
            );
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(&notice.id),
                &content,
                Some(&trigger),
            );
            let (tx, rx) = mpsc::channel();
            let block = RcBlock::new(move |_error: *mut NSError| {
                let _ = tx.send(());
            });
            center.addNotificationRequest_withCompletionHandler(&request, Some(&block));
            let _ = rx.recv_timeout(WAIT);
        }
    }

    /// Every request pending and every notification delivered, as text, for
    /// a check: only this app's own bundle can read them.
    pub fn listing() -> String {
        let Some(center) = center() else {
            return "not an app bundle: no notifications".into();
        };
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |list: NonNull<NSArray<UNNotificationRequest>>| {
            let lines = unsafe { list.as_ref() }
                .iter()
                .map(|request| {
                    let content = request.content();
                    format!(
                        "pending {} \"{}: {}\"",
                        request.identifier(),
                        content.title(),
                        content.body()
                    )
                })
                .collect::<Vec<_>>();
            let _ = tx.send(lines);
        });
        center.getPendingNotificationRequestsWithCompletionHandler(&block);
        let mut lines = rx.recv_timeout(WAIT).unwrap_or_default();
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(
            move |list: NonNull<NSArray<objc2_user_notifications::UNNotification>>| {
                let found = unsafe { list.as_ref() }
                    .iter()
                    .map(|n| {
                        let content = n.request().content();
                        format!(
                            "delivered {} at {} \"{}: {}\"",
                            n.request().identifier(),
                            n.date().description(),
                            content.title(),
                            content.body()
                        )
                    })
                    .collect::<Vec<_>>();
                let _ = tx.send(found);
            },
        );
        center.getDeliveredNotificationsWithCompletionHandler(&block);
        lines.extend(rx.recv_timeout(WAIT).unwrap_or_default());
        lines.insert(0, format!("permission {:?}", permission()));
        lines.join("\n")
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and this class does not implement Drop.
        #[unsafe(super = NSObject)]
        #[name = "MagnesiumNotificationDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                done: &block2::DynBlock<dyn Fn()>,
            ) {
                let info = response.notification().request().content().userInfo();
                let key = NSString::from_str(FOLDER_KEY);
                let key: &AnyObject = &key;
                let folder = info
                    .objectForKey(key)
                    .and_then(|value| value.downcast::<NSString>().ok())
                    .map(|value| value.to_string());
                if let Some(folder) = folder {
                    super::opened(folder);
                }
                done.call(());
            }
        }
    );

    /// Hear clicks on the notification. Set while the app starts, so a click
    /// that starts the app is heard too.
    pub fn listen() {
        let Some(center) = center() else {
            return;
        };
        let delegate: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(), init] };
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // The centre holds its delegate weakly: this one lives as long as the app.
        std::mem::forget(delegate);
    }
}

#[cfg(target_os = "macos")]
pub use mac::{bundle_id, listing, packaged, pending, permission, replace};

#[cfg(target_os = "macos")]
pub fn ask() -> Permission {
    mac::ask(std::env::var_os(PROVISIONAL_ENV).is_some_and(|v| !v.is_empty()))
}

#[cfg(target_os = "macos")]
pub fn listen(app: AppHandle) {
    let _ = APP.set(app);
    mac::listen();
}

#[cfg(not(target_os = "macos"))]
pub fn pending() -> Option<Vec<String>> {
    None
}
#[cfg(not(target_os = "macos"))]
pub fn listing() -> String {
    "no notifications on this system".into()
}
#[cfg(not(target_os = "macos"))]
pub fn bundle_id() -> Option<String> {
    None
}
#[cfg(not(target_os = "macos"))]
pub fn packaged() -> bool {
    false
}
#[cfg(not(target_os = "macos"))]
pub fn permission() -> Permission {
    Permission::Unavailable
}
#[cfg(not(target_os = "macos"))]
pub fn ask() -> Permission {
    Permission::Unavailable
}
#[cfg(not(target_os = "macos"))]
pub fn replace(_notices: &[Notice]) {}
#[cfg(not(target_os = "macos"))]
pub fn listen(app: AppHandle) {
    let _ = APP.set(app);
}
