// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod android;
mod browser_extension;
mod chrome;
mod json;
mod minecraft;
mod mozilla;
mod po;

use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Formatter},
    sync::Arc,
    vec,
};

use async_trait::async_trait;
use reqwest::Client;
use unic_langid::LanguageIdentifier;

pub use self::{
    android::AndroidProvider,
    browser_extension::BrowserExtensionProvider,
    chrome::ChromeProvider,
    json::JsonProvider,
    minecraft::MinecraftProvider,
    mozilla::MozillaProvider,
    po::gnome::graphql_gnome,
    po::kde::graphql_kde,
    po::{NetPoProvider, PoProvider},
};
use super::Translation;

#[async_trait]
pub trait TranslationProvider {
    fn id(&self) -> &str;

    fn name(&self) -> &str;

    fn group_name(&self) -> Option<&str> {
        None
    }

    async fn generate(
        &self,
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> Result<BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error>;
}

impl Debug for dyn TranslationProvider + Send + Sync {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "provider({})", self.id())
    }
}

/// Returns a string version of `lang_id`.
///
/// # Examples
/// ```ignore
/// assert!(
///     lang_id_to_string("ca_ES_valencia".parse().unwrap(), "-", false, "@", true),
///     String::from("ca-es@VALENCIA"),
/// );
/// ```
fn lang_id_to_string(
    lang_id: &LanguageIdentifier,
    region_binder: &str,
    uppercase_region: bool,
    variant_binder: &str,
    uppercase_variant: bool,
) -> String {
    let mut s = lang_id.language.to_string();
    if let Some(region) = &lang_id.region {
        s.push_str(region_binder);
        if uppercase_region {
            s.push_str(region.as_str());
        } else {
            s.push_str(&region.as_str().to_lowercase());
        }
    }
    for variant in lang_id.variants() {
        s.push_str(variant_binder);
        if uppercase_variant {
            s.push_str(&variant.as_str().to_uppercase());
        } else {
            s.push_str(variant.as_str());
        }
    }
    s
}

macro_rules! android {
    ($name:literal, github => $repo:literal) => {
        android!($name, github => $repo, "strings")
    };
    ($name:literal, github => $repo:literal, $file_name:literal) => {
        Arc::new(AndroidProvider {
            id: concat!("github/", $repo, "/", $file_name),
            name: $name,
            group_name: Some("Android apps"),
            decode_as_base64: false,
            default_url: concat!(
                "https://raw.githubusercontent.com/",
                $repo,
                "/master/app/src/main/res/values/",
                $file_name,
                ".xml"
            ),
            url: |lang_id| {
                format!(
                    concat!(
                        "https://raw.githubusercontent.com/",
                        $repo,
                        "/master/app/src/main/res/values-{}/",
                        $file_name,
                        ".xml"
                    ),
                    lang_id_to_string(&lang_id, "-r", true, "-", false),
                )
            },
        })
    };
    ($name:literal, gitlab => $repo:literal) => {
        Arc::new(AndroidProvider {
            id: concat!("gitlab/", $repo),
            name: $name,
            group_name: Some("Android apps"),
            decode_as_base64: false,
            default_url: concat!(
                "https://gitlab.com/",
                $repo,
                "/-/raw/master/app/src/main/res/values/strings.xml",
            ),
            url: |lang_id| {
                format!(
                    concat!(
                        "https://gitlab.com/",
                        $repo,
                        "/-/raw/master/app/src/main/res/values-{}/strings.xml",
                    ),
                    lang_id_to_string(&lang_id, "-r", true, "-", false),
                )
            },
        })
    };
    ($name:literal, source => $repo:literal, $folder:literal) => {
        Arc::new(AndroidProvider {
            id: concat!("android/", $repo, "/", $folder),
            name: $name,
            group_name: Some("Android"),
            decode_as_base64: true,
            default_url: concat!(
                "https://android.googlesource.com/platform/",
                $repo,
                "/+/master/",
                $folder,
                "/values/strings.xml?format=TEXT",
            ),
            url: |lang_id| {
                format!(
                    concat!(
                        "https://android.googlesource.com/platform/",
                        $repo,
                        "/+/master/",
                        $folder,
                        "/values-{}/strings.xml?format=TEXT",
                    ),
                    lang_id_to_string(&lang_id, "-r", true, "-", false),
                )
            },
        })
    };
}

macro_rules! browser_extension {
    ($name:literal, github => $repo:literal, $folder:literal) => {
        Arc::new(BrowserExtensionProvider {
            id: concat!("github/", $repo, "/", $folder),
            name: $name,
            group_name: Some("Browser extension"),
            default_lang: "en",
            url: |lang_id| {
                format!(
                    concat!(
                        "https://github.com/",
                        $repo,
                        "/raw/",
                        $folder,
                        "/{}/messages.json",
                    ),
                    lang_id_to_string(&lang_id, "_", true, "_", false),
                )
            },
        })
    };
    ($name:literal, gitlab => $repo:literal, $folder:literal) => {
        Arc::new(BrowserExtensionProvider {
            id: concat!("gitlab/", $repo, "/", $folder),
            name: $name,
            group_name: Some("Browser extension"),
            default_lang: "en_US",
            url: |lang_id| {
                format!(
                    concat!("https://", $repo, "/-/raw/", $folder, "/{}/messages.json",),
                    lang_id_to_string(&lang_id, "_", true, "_", false),
                )
            },
        })
    };
}

macro_rules! po {
    ($id:literal, $name:literal, $group_name:expr, $remove_char:expr, github => $path:literal) => {
        Arc::new(PoProvider {
            id: $id,
            name: $name,
            group_name: $group_name,
            url: |lang_id| {
                format!(
                    concat!("https://raw.githubusercontent.com/", $path),
                    lang_id_to_string(lang_id, "_", true, "_", false),
                )
            },
            remove_char: $remove_char,
        })
    };
    ($name:literal, elementary => $repo:literal, $path:literal) => {
        po!($name, $repo, elementary => $repo, $path)
    };
    ($name:literal, $id:literal, elementary => $repo:literal, $path:literal) => {
        Arc::new(PoProvider {
            id: concat!("elementary-", $id),
            name: $name,
            group_name: Some("Elementary"),
            url: |lang_id| {
                format!(
                    concat!(
                        "https://raw.githubusercontent.com/elementary/",
                        $repo,
                        "/",
                        $path,
                        "/{}.po",
                    ),
                    lang_id_to_string(lang_id, "_", true, "_", false),
                )
            },
            remove_char: Some('_'),
        })
    };
}

macro_rules! json {
    (elementary => $path:literal) => {
        Arc::new(JsonProvider {
            id: concat!("elementary-web-", $path),
            name: $path,
            group_name: Some("Elementary"),
            url: |lang_id| {
                format!(
                    concat!(
                        "https://raw.githubusercontent.com/elementary/website/master/_lang/{}/",
                        $path,
                        ".json",
                    ),
                    lang_id_to_string(lang_id, "_", true, "@", false),
                )
            },
        })
    };
}

#[rustfmt::skip]
pub fn default_providers() -> Vec<Arc<dyn TranslationProvider + Send + Sync>> {
    let providers: Vec<Arc<dyn TranslationProvider + Send + Sync>> = vec![
        Arc::new(ChromeProvider),
        Arc::new(MinecraftProvider),
        Arc::new(MozillaProvider),
        Arc::new(NetPoProvider {
            id: "gnome",
            name: "GNOME",
            urls: graphql_gnome,
            remove_char: Some('_'),
        }),
        Arc::new(NetPoProvider {
            id: "kde",
            name: "KDE",
            urls: graphql_kde,
            remove_char: Some('&'),
        }),

        po!("multimc",   "MultiMC",            None,            Some('&'), github => "MultiMC/Translations/master/{}.po"),
        po!("weblate",   "Weblate",            Some("Weblate"), None,      github => "WeblateOrg/weblate/main/weblate/locale/{}/LC_MESSAGES/django.po"),
        po!("weblatejs", "Weblate JavaScript", Some("Weblate"), None,      github => "WeblateOrg/weblate/main/weblate/locale/{}/LC_MESSAGES/djangojs.po"),

        Arc::new(PoProvider {
            id: "pacman",
            name: "Pacman",
            group_name: None,
            url: |lang_id| format!(
                "https://gitlab.archlinux.org/pacman/pacman/-/raw/master/src/pacman/po/{}.po",
                lang_id_to_string(lang_id, "_", true, "@", false),
            ),
            remove_char: None,
        }),

        po!("AppCenter",                              elementary => "appcenter",   "master/po"),
        po!("AppCenter Extra",   "appcenter-extra",   elementary => "appcenter",   "master/po/extra"),
        po!("Calculator",                             elementary => "calculator",  "master/po"),
        po!("Calculator Extra",  "calculator-extra",  elementary => "calculator",  "master/po/extra"),
        po!("Calendar",                               elementary => "calendar",    "master/po"),
        po!("Calendar Extra",    "calendar-extra",    elementary => "calendar",    "master/po/extra"),
        po!("Camera",                                 elementary => "camera",      "master/po"),
        po!("Camera Extra",      "camera-extra",      elementary => "camera",      "master/po/extra"),
        po!("Code",                                   elementary => "code",        "master/po"),
        po!("Code Extra",        "code-extra",        elementary => "code",        "master/po/extra"),
        po!("Code Plugins",      "code-plugins",      elementary => "code",        "master/po/plugins"),
        po!("Files",                                  elementary => "files",       "main/po"),
        po!("Files Extra",       "files-extra",       elementary => "files",       "main/po/extra"),
        po!("Friends",                                elementary => "friends",     "master/po"),
        po!("Friends Extra",     "friends-extra",     elementary => "friends",     "master/po/extra"),
        po!("Installer",                              elementary => "installer",   "master/po"),
        po!("Installer Extra",   "installer-extra",   elementary => "installer",   "master/po/extra"),
        po!("Mail",                                   elementary => "mail",        "master/po"),
        po!("Mail Extra",        "mail-extra",        elementary => "mail",        "master/po/extra"),
        po!("Music",                                  elementary => "music",       "main/po"),
        po!("Music Extra",       "music-extra",       elementary => "music",       "main/po/extra"),
        po!("Photos",                                 elementary => "photos",      "master/po"),
        po!("Photos Extra",      "photos-extra",      elementary => "photos",      "master/po/extra"),
        po!("Screenshot",                             elementary => "screenshot",  "master/po"),
        po!("Screenshot Extra",  "screenshot-extra",  elementary => "screenshot",  "master/po/extra"),
        po!("Switchboard",                            elementary => "switchboard", "main/po"),
        po!("Switchboard Extra", "switchboard-extra", elementary => "switchboard", "main/po/extra"),
        po!("Tasks",                                  elementary => "tasks",       "master/po"),
        po!("Tasks Extra",       "tasks-extra",       elementary => "tasks",       "master/po/extra"),
        po!("Terminal",                               elementary => "terminal",    "master/po"),
        po!("Terminal Extra",    "terminal-extra",    elementary => "terminal",    "master/po/extra"),
        po!("Videos",                                 elementary => "videos",      "main/po"),
        po!("Videos Extra",      "videos-extra",      elementary => "videos",      "main/po/extra"),
        po!("Wingpanel",                              elementary => "wingpanel",   "master/po"),
        po!("Wingpanel Extra",   "wingpanel-extra",   elementary => "wingpanel",   "master/po/extra"),

        po!("Captive Network Assistant",                                         elementary => "capnet-assist",         "master/po"),
        po!("Captive Network Assistant Extra",    "capnet-assist-extra",         elementary => "capnet-assist",         "master/po/extra"),
        po!("Feedback",                                                          elementary => "feedback",              "master/po"),
        po!("Feedback Extra",                     "feedback-extra",              elementary => "feedback",              "master/po/extra"),
        po!("Flatpak Platform",                                                  elementary => "flatpak-platform",      "main/platform-data/po"),
        po!("Gala",                                                              elementary => "gala",                  "master/po"),
        po!("Granite",                                                           elementary => "granite",               "main/po"),
        po!("Granite Extra",                      "granite-extra",               elementary => "granite",               "main/po/extra"),
        po!("Greeter",                                                           elementary => "greeter",               "master/po"),
        po!("Greeter Extra",                      "greeter-extra",               elementary => "greeter",               "master/po/extra"),
        po!("Icons",                                                             elementary => "icons",                 "main/po"),
        po!("Notifications",                                                     elementary => "notifications",         "master/po/extra"),
        po!("Pantheon Polkit Agent",                                             elementary => "pantheon-agent-polkit", "main/po"),
        po!("Pantheon Polkit Agent Extra",        "pantheon-agent-polkit-extra", elementary => "pantheon-agent-polkit", "main/po/extra"),
        po!("Pantheon XDG Desktop Portals",                                      elementary => "portals",               "main/po"),
        po!("Pantheon XDG Desktop Portals Extra", "portals-extra",               elementary => "portals",               "main/po/extra"),
        po!("Settings Daemon",                                                   elementary => "settings-daemon",       "master/po"),
        po!("Shortcut Overlay",                                                  elementary => "shortcut-overlay",      "master/po"),
        po!("Shortcut Overlay Extra",             "shortcut-overlay-extra",      elementary => "shortcut-overlay",      "master/po/extra"),
        po!("Sideload",                                                          elementary => "sideload",              "master/po"),
        po!("Sideload Extra",                     "sideload-extra",              elementary => "sideload",              "master/po/extra"),
        po!("Stylesheet",                                                        elementary => "stylesheet",            "master/po"),
        po!("Wallpapers",                                                        elementary => "wallpapers",            "master/po"),

        json!(elementary => "docs/installation"),
        json!(elementary => "docs/learning-the-basics"),
        json!(elementary => "docs/translation-guide"),
        json!(elementary => "store/cart"),
        json!(elementary => "store/index"),
        json!(elementary => "403"),
        json!(elementary => "404"),
        json!(elementary => "410"),
        json!(elementary => "SECURITY"),
        json!(elementary => "brand"),
        json!(elementary => "capnet-assist"),
        json!(elementary => "code-of-conduct"),
        json!(elementary => "get-involved"),
        json!(elementary => "index"),
        json!(elementary => "layout"),
        json!(elementary => "oem"),
        json!(elementary => "open-source"),
        json!(elementary => "press"),
        json!(elementary => "privacy"),
        json!(elementary => "support"),
        json!(elementary => "thank-you"),

        browser_extension!("Decentraleyes",       gitlab => "git.synz.io/Synzvato/decentraleyes", "master/_locales"),
        browser_extension!("ImprovedTube",        github => "code-charity/youtube",               "master/_locales"),
        browser_extension!("Midnight Lizard",     github => "Midnight-Lizard/Midnight-Lizard",    "master/_locales"),
        browser_extension!("Simple Translate",    github => "sienori/simple-translate",           "master/src/_locales"),
        browser_extension!("Tab Session Manager", github => "sienori/Tab-Session-Manager",        "master/src/_locales"),
        browser_extension!("Tampermonkey",        github => "Tampermonkey/tampermonkey",          "master/i18n"),
        browser_extension!("Tree Style Tab",      github => "piroor/treestyletab",                "trunk/webextensions/_locales"),
        browser_extension!("Turn Off The Lights", github => "turnoffthelights/Turn-Off-the-Lights-Chrome-extension", "master/src/_locales"),
        browser_extension!("uBlock Origin",       github => "gorhill/uBlock",                     "master/src/_locales"),

        android!("Etar",                      github => "Etar-Group/Etar-Calendar"),
        android!("F-Droid",                   gitlab => "fdroid/fdroidclient"),
        android!("Material Files",            github => "zhanghai/MaterialFiles"),
        android!("Material Files mime types", github => "zhanghai/MaterialFiles", "mime_types"),
        android!("Notally",                   github => "OmGodse/Notally"),

        android!("", source => "bootable/recovery",                         "tools/recovery_l10n/res"),
        android!("", source => "development",                               "apps/Fallback/res"),
        android!("", source => "frameworks/base",                           "core/res/res"),
        android!("", source => "frameworks/base",                           "libs/WindowManager/Shell/res"),
        android!("", source => "frameworks/base",                           "packages/BackupRestoreConfirmation/res"),
        android!("", source => "frameworks/base",                           "packages/CarrierDefaultApp/res"),
        android!("", source => "frameworks/base",                           "packages/CompanionDeviceManager/res"),
        android!("", source => "frameworks/base",                           "packages/DynamicSystemInstallationService/res"),
        android!("", source => "frameworks/base",                           "packages/ExternalStorageProvider/res"),
        android!("", source => "frameworks/base",                           "packages/FusedLocation/res"),
        android!("", source => "frameworks/base",                           "packages/InputDevices/res"),
        android!("", source => "frameworks/base",                           "packages/PackageInstaller/res"),
        android!("", source => "frameworks/base",                           "packages/PrintSpooler/res"),
        android!("", source => "frameworks/base",                           "packages/SettingsLib/BannerMessagePreference/res"),
        android!("", source => "frameworks/base",                           "packages/SettingsLib/FooterPreference/res"),
        android!("", source => "frameworks/base",                           "packages/SettingsLib/HelpUtils/res"),
        android!("", source => "frameworks/base",                           "packages/SettingsLib/RestrictedLockUtils/res"),
        android!("", source => "frameworks/base",                           "packages/SettingsLib/SearchWidget/res"),
        android!("", source => "frameworks/base",                           "packages/SettingsLib/SelectorWithWidgetPreference/res"),
        android!("", source => "frameworks/base",                           "packages/SettingsLib/res"),
        android!("", source => "frameworks/base",                           "packages/SettingsProvider/res"),
        android!("", source => "frameworks/base",                           "packages/Shell/res"),
        android!("", source => "frameworks/base",                           "packages/SimAppDialog/res"),
        android!("", source => "frameworks/base",                           "packages/SoundPicker/res"),
        android!("", source => "frameworks/base",                           "packages/SystemUI/res-keyguard"),
        android!("", source => "frameworks/base",                           "packages/SystemUI/res-product"),
        android!("", source => "frameworks/base",                           "packages/SystemUI/res"),
        android!("", source => "frameworks/base",                           "packages/VpnDialogs/res"),
        android!("", source => "frameworks/base",                           "packages/WallpaperCropper/res"),
        android!("", source => "frameworks/base",                           "packages/overlays/AvoidAppsInCutoutOverlay/res"),
        android!("", source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationCornerOverlay/res"),
        android!("", source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationDoubleOverlay/res"),
        android!("", source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationHoleOverlay/res"),
        android!("", source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationNarrowOverlay/res"),
        android!("", source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationTallOverlay/res"),
        android!("", source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationWaterfallOverlay/res"),
        android!("", source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationWideOverlay/res"),
        android!("", source => "frameworks/base",                           "packages/overlays/NoCutoutOverlay/res"),
        android!("", source => "frameworks/opt/chips",                      "res"),
        android!("", source => "frameworks/opt/chips",                      "sample/res"),
        android!("", source => "frameworks/opt/colorpicker",                "res"),
        android!("", source => "frameworks/opt/net/wifi",                   "libs/WifiTrackerLib/res"),
        android!("", source => "frameworks/opt/photoviewer",                "res"),
        android!("", source => "frameworks/opt/photoviewer",                "sample/res"),
        android!("", source => "frameworks/opt/setupwizard",                "library/main/res"),
        android!("", source => "frameworks/opt/timezonepicker",             "res"),
        android!("", source => "packages/apps/BasicSmsReceiver",            "res"),
        android!("", source => "packages/apps/Calendar",                    "res"),
        android!("", source => "packages/apps/Camera2",                     "res"),
        android!("", source => "packages/apps/Car/Calendar",                "res"),
        android!("", source => "packages/apps/Car/Launcher",                "res"),
        android!("", source => "packages/apps/Car/LinkViewer",              "res"),
        android!("", source => "packages/apps/Car/Notification",            "res"),
        android!("", source => "packages/apps/Car/Settings",                "res"),
        android!("", source => "packages/apps/Car/SystemUI",                "res"),
        android!("", source => "packages/apps/Car/SystemUpdater",           "res"),
        android!("", source => "packages/apps/Car/systemlibs",              "car-assist-client-lib/res"),
        android!("", source => "packages/apps/Car/systemlibs",              "car-broadcastradio-support/res"),
        android!("", source => "packages/apps/CellBroadcastReceiver",       "res"),
        android!("", source => "packages/apps/CertInstaller",               "res"),
        android!("", source => "packages/apps/Contacts",                    "res"),
        android!("", source => "packages/apps/DeskClock",                   "res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/about/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/app/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/assisteddialing/ui/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/blocking/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/blockreportspam/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/callcomposer/cameraui/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/callcomposer/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/calldetails/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/calllog/ui/menu/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/calllog/ui/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/calllogutils/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/clipboard/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/common/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/contactphoto/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/dialpadview/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/glidephotomanager/impl/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/historyitemactions/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/interactions/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/main/impl/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/notification/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/phonenumberutil/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/postcall/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/precall/impl/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/preferredsim/impl/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/preferredsim/suggestion/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/promotion/impl/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/cp2/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/directories/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/list/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/nearbyplaces/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/shortcuts/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/spam/promo/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/spannable/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/speeddial/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/theme/common/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/util/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/voicemail/listui/error/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/voicemail/settings/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/dialer/widget/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/answer/impl/answermethod/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/answer/impl/hint/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/answer/impl/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/audioroute/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/commontheme/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/contactgrid/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/disconnectdialog/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/hold/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/incall/impl/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/rtt/impl/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/sessiondata/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/spam/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/telecomeventui/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/theme/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/incallui/video/impl/res"),
        android!("", source => "packages/apps/Dialer",                      "java/com/android/voicemail/impl/res"),
        android!("", source => "packages/apps/DocumentsUI",                 "res"),
        android!("", source => "packages/apps/EmergencyInfo",               "EmergencyGestureAction/res"),
        android!("", source => "packages/apps/EmergencyInfo",               "res"),
        android!("", source => "packages/apps/Gallery",                     "res"),
        android!("", source => "packages/apps/Gallery2",                    "res"),
        android!("", source => "packages/apps/HTMLViewer",                  "res"),
        android!("", source => "packages/apps/KeyChain",                    "res"),
        android!("", source => "packages/apps/Launcher3",                   "go/quickstep/res"),
        android!("", source => "packages/apps/Launcher3",                   "quickstep/res"),
        android!("", source => "packages/apps/Launcher3",                   "res"),
        android!("", source => "packages/apps/LegacyCamera",                "res"),
        android!("", source => "packages/apps/ManagedProvisioning",         "res"),
        android!("", source => "packages/apps/Messaging",                   "res"),
        android!("", source => "packages/apps/Music",                       "kotlin/res"),
        android!("", source => "packages/apps/MusicFX",                     "res"),
        android!("", source => "packages/apps/Nfc",                         "res"),
        android!("", source => "packages/apps/PhoneCommon",                 "res"),
        android!("", source => "packages/apps/Protips",                     "res"),
        android!("", source => "packages/apps/QuickAccessWallet",           "res"),
        android!("", source => "packages/apps/SafetyRegulatoryInfo",        "res"),
        android!("", source => "packages/apps/Settings",                    "res"),
        android!("", source => "packages/apps/SettingsIntelligence",        "res"),
        android!("", source => "packages/apps/Stk",                         "res"),
        android!("", source => "packages/apps/StorageManager",              "res"),
        android!("", source => "packages/apps/TV",                          "common/res"),
        android!("", source => "packages/apps/TV",                          "res"),
        android!("", source => "packages/apps/Tag",                         "res"),
        android!("", source => "packages/apps/ThemePicker",                 "res"),
        android!("", source => "packages/apps/Traceur",                     "res"),
        android!("", source => "packages/apps/TvSettings",                  "Settings/res-twopanel"),
        android!("", source => "packages/apps/TvSettings",                  "Settings/res"),
        android!("", source => "packages/apps/TvSettings",                  "TwoPanelSettingsLib/res"),
        android!("", source => "packages/apps/WallpaperPicker",             "res"),
        android!("", source => "packages/apps/WallpaperPicker2",            "res"),
        android!("", source => "packages/inputmethods/LatinIME",            "java/res"),
        android!("", source => "packages/inputmethods/LeanbackIME",         "res"),
        android!("", source => "packages/modules/Bluetooth",                "android/app/res"),
        android!("", source => "packages/modules/CaptivePortalLogin",       "res"),
        android!("", source => "packages/modules/CellBroadcastService",     "res"),
        android!("", source => "packages/modules/Connectivity",             "Tethering/res"),
        android!("", source => "packages/modules/Connectivity",             "service/ServiceConnectivityResources/res"),
        android!("", source => "packages/modules/ExtServices",              "java/res"),
        android!("", source => "packages/modules/NetworkStack",             "res"),
        android!("", source => "packages/modules/Permission",               "PermissionController/res"),
        android!("", source => "packages/modules/Permission",               "SafetyCenter/Resources/res"),
        android!("", source => "packages/modules/Wifi",                     "OsuLogin/res"),
        android!("", source => "packages/modules/Wifi",                     "service/ServiceWifiResources/res"),
        android!("", source => "packages/providers/BlockedNumberProvider",  "res"),
        android!("", source => "packages/providers/CalendarProvider",       "res"),
        android!("", source => "packages/providers/ContactsProvider",       "res"),
        android!("", source => "packages/providers/DownloadProvider",       "res"),
        android!("", source => "packages/providers/DownloadProvider",       "ui/res"),
        android!("", source => "packages/providers/MediaProvider",          "res"),
        android!("", source => "packages/providers/TelephonyProvider",      "res"),
        android!("", source => "packages/providers/TvProvider",             "res"),
        android!("", source => "packages/providers/UserDictionaryProvider", "res"),
        android!("", source => "packages/screensavers/Basic",               "res"),
        android!("", source => "packages/screensavers/PhotoTable",          "res"),
        android!("", source => "packages/services/BuiltInPrintService",     "res"),
        android!("", source => "packages/services/Car",                     "FrameworkPackageStubs/res"),
        android!("", source => "packages/services/Car",                     "car-admin-ui-lib/src/main/res"),
        android!("", source => "packages/services/Car",                     "car-maps-placeholder/res"),
        android!("", source => "packages/services/Car",                     "car-usb-handler/res"),
        android!("", source => "packages/services/Car",                     "car_product/car_ui_portrait/apps/CarUiPortraitSystemUI/res"),
        // The commented translations has no default translation
        // android!("", source => "packages/services/Car",                  "car_product/car_ui_portrait/rro/CarEvsCameraPreviewAppRRO/res"),
        // android!("", source => "packages/services/Car",                  "car_product/car_ui_portrait/rro/CarUiPortraitDialerRRO/res"),
        // android!("", source => "packages/services/Car",                  "car_product/car_ui_portrait/rro/CarUiPortraitNotificationRRO/res"),
        android!("", source => "packages/services/Car",                     "car_product/overlay/frameworks/base/core/res/res"),
        android!("", source => "packages/services/Car",                     "experimental/service/res"),
        android!("", source => "packages/services/Car",                     "packages/CarDeveloperOptions/res"),
        android!("", source => "packages/services/Car",                     "packages/CarManagedProvisioning/res"),
        android!("", source => "packages/services/Car",                     "service-builtin/res"),
        android!("", source => "packages/services/Car",                     "service/res"),
        android!("", source => "packages/services/Car",                     "tests/BugReportApp/res"),
        android!("", source => "packages/services/Car",                     "tests/DiagnosticTools/res"),
        android!("", source => "packages/services/Car",                     "tests/MultiDisplaySecondaryHomeTestLauncher/res"),
        android!("", source => "packages/services/Car",                     "tests/MultiDisplayTest/res"),
        android!("", source => "packages/services/Car",                     "tests/MultiDisplayTestHelloActivity/res"),
        android!("", source => "packages/services/Mtp",                     "res"),
        android!("", source => "packages/services/Telecomm",                "res"),
        android!("", source => "packages/services/Telephony",               "res"),
        android!("", source => "packages/services/Telephony",               "testapps/GbaTestApp/res"),
        android!("", source => "packages/services/Telephony",               "testapps/TestSliceApp/app/src/main/res"),
        android!("", source => "packages/wallpapers/LivePicker",            "res"),
    ];

    #[cfg(debug_assertions)]
    {
        use std::collections::HashSet;
        
        let mut set = HashSet::with_capacity(providers.len());
        for provider in &providers {
            if let Some(group_name) = provider.group_name() {
                set.insert(group_name);
            }
        }
        for (i, provider) in providers.iter().enumerate() {
            if set.contains(provider.id()) {
                panic!("Duplicate id: {}, second at index {i}", provider.id());
            }
            // if set.contains(provider.name()) {
            //     panic!("Duplicate name: {}, second at index {i}", provider.name());
            // }
            // set.insert(provider.name());
        }
    }

    providers
}
