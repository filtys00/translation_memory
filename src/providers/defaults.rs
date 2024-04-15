// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{sync::Arc, vec};

use super::{
    android::{parse_android, parse_android_base64},
    browser_extension::parse_browser_extension,
    chrome::ChromeProvider,
    dtd::parse_dtd,
    json::{parse_freeshow_json, parse_json},
    lang_id_to_string,
    minecraft::MinecraftProvider,
    mozilla::MozillaProvider,
    po::{gnome::graphql_gnome, kde::graphql_kde, parse_po, NetPoProvider},
    properties::parse_properties,
    srt::parse_srt,
    DuoProvider, MonoProvider, TranslationProvider,
};

macro_rules! android {
    ($id:literal, $name:literal, github => $repo:literal) => {
        android!($id, $name, github => $repo, "strings")
    };
    ($id:literal, $name:literal, github => $repo:literal, $file_name:literal) => {
        Arc::new(DuoProvider {
            id: $id,
            name: $name,
            group_name: Some("Android apps"),
            parse: parse_android,
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
    ($id:literal, $name:literal, gitlab => $repo:literal) => {
        Arc::new(DuoProvider {
            id: $id,
            name: $name,
            group_name: Some("Android apps"),
            parse: parse_android,
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

    (source => $repo:literal, $folder:literal) => {
        Arc::new(DuoProvider {
            id: concat!("android-", $repo, "-", $folder),
            name: "",
            group_name: Some("Android"),
            parse: parse_android_base64,
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

    ($id:literal, $name:literal, tor => $branch:literal, $default_path:literal, $path:literal) => {
        Arc::new(DuoProvider {
            id: concat!("torproject-", $id),
            name: $name,
            group_name: Some("The Tor Project"),
            parse: parse_android,
            default_url: concat!(
                "https://gitlab.torproject.org/tpo/translation/-/raw/",
                $branch,
                "/",
                $default_path,
            ),
            url: |lang_id| {
                format!(
                    concat!(
                        "https://gitlab.torproject.org/tpo/translation/-/raw/",
                        $branch,
                        "/",
                        $path,
                    ),
                    lang_id_to_string(&lang_id, "-r", true, "-", false),
                )
            },
        })
    };
}

macro_rules! browser_extension {
    ($id:literal, $name:literal, github => $repo:literal, $folder:literal) => {
        Arc::new(DuoProvider {
            id: $id,
            name: $name,
            group_name: Some("Browser extension"),
            parse: parse_browser_extension,
            default_url: concat!(
                "https://github.com/",
                $repo,
                "/raw/",
                $folder,
                "/en/messages.json",
            ),
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
    ($id:literal, $name:literal, gitlab => $repo:literal, $folder:literal) => {
        Arc::new(DuoProvider {
            id: $id,
            name: $name,
            group_name: Some("Browser extension"),
            parse: parse_browser_extension,
            default_url: concat!(
                "https://",
                $repo,
                "/-/raw/",
                $folder,
                "/en_US/messages.json"
            ),
            url: |lang_id| {
                format!(
                    concat!("https://", $repo, "/-/raw/", $folder, "/{}/messages.json"),
                    lang_id_to_string(&lang_id, "_", true, "_", false),
                )
            },
        })
    };

    ($id:literal, $name:literal, tor => $branch:literal, $file_name:literal) => {
        Arc::new(DuoProvider {
            id: concat!("torproject-", $id),
            name: $name,
            group_name: Some("The Tor Project"),
            parse: parse_browser_extension,
            default_url: concat!(
                "https://gitlab.torproject.org/tpo/translation/-/raw/",
                $branch,
                "/en_US/",
                $file_name,
                ".json",
            ),
            url: |lang_id| {
                format!(
                    concat!(
                        "https://gitlab.torproject.org/tpo/translation/-/raw/",
                        $branch,
                        "/{}/",
                        $file_name,
                        ".json",
                    ),
                    lang_id_to_string(&lang_id, "_", true, "_", false),
                )
            },
        })
    };
}

macro_rules! dtd {
    ($id:literal, $name:literal, tor => $branch:literal, $file_name:literal) => {
        Arc::new(DuoProvider {
            id: concat!("torproject-", $id),
            name: $name,
            group_name: Some("The Tor Project"),
            parse: parse_dtd,
            default_url: concat!(
                "https://gitlab.torproject.org/tpo/translation/-/raw/",
                $branch,
                "/en-US/",
                $file_name,
                ".dtd",
            ),
            url: |lang_id| {
                format!(
                    concat!(
                        "https://gitlab.torproject.org/tpo/translation/-/raw/",
                        $branch,
                        "/{}/",
                        $file_name,
                        ".dtd",
                    ),
                    lang_id_to_string(lang_id, "-", true, "@", false),
                )
            },
        })
    };
}

macro_rules! json {
    ($id:literal, elementary => $path:literal) => {
        Arc::new(MonoProvider {
            id: concat!("elementary-", $id),
            name: concat!("Elementary ", $path),
            group_name: Some("Elementary"),
            parse: parse_json,
            remove_char: None,
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

macro_rules! po {
    ($id:literal, $name:literal, $group_name:expr, $remove_char:expr, github => $path:literal) => {
        po!($id, $name, $group_name, $remove_char, "@", false, github => $path)
    };
    (
        $id:literal, $name:literal, $group_name:expr, $remove_char:expr,
        $variant_binder:expr, $uppercase_variant:expr,
        github => $path:literal
    ) => {
        po!(
            $id, $name, $group_name, $remove_char, $variant_binder, $uppercase_variant,
            concat!("https://raw.githubusercontent.com/", $path),
        )
    };

    ($id:literal, $name:literal, $group_name:expr, $remove_char:expr, gitlab => $site:literal, $repo:literal, $path:literal) => {
        po!($id, $name, $group_name, $remove_char, "@", false, gitlab => $site, $repo, $path)
    };
    (
        $id:literal, $name:literal, $group_name:expr, $remove_char:expr,
        $variant_binder:expr, $uppercase_variant:expr,
        gitlab => $site:literal, $repo:literal, $path:literal
    ) => {
        po!(
            $id, $name, $group_name, $remove_char, $variant_binder, $uppercase_variant,
            concat!("https://", $site, "/", $repo, "/-/raw/", $path),
        )
    };

    (
        $id:literal, $name:literal, $group_name:expr, $remove_char:expr,
        $variant_binder:literal, $uppercase_variant:literal,
        $url:expr,
    ) => {
        Arc::new(MonoProvider {
            id: $id,
            name: $name,
            group_name: $group_name,
            parse: parse_po,
            remove_char: $remove_char,
            url: |lang_id| {
                format!($url, lang_id_to_string(lang_id, "_", true, $variant_binder, $uppercase_variant))
            },
        })
    };

    ($id:literal, $name:literal, elementary => $repo:literal, $path:literal) => {
        Arc::new(MonoProvider {
            id: concat!("elementary-", $id),
            name: $name,
            group_name: Some("Elementary"),
            parse: parse_po,
            remove_char: Some('_'),
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
        })
    };

    ($id:literal, $name:literal, tor => $branch:literal, $path:literal) => {
        Arc::new(MonoProvider {
            id: concat!("torproject-", $id),
            name: $name,
            group_name: Some("The Tor Project"),
            parse: parse_po,
            remove_char: None,
            url: |lang_id| {
                format!(
                    concat!(
                        "https://gitlab.torproject.org/tpo/translation/-/raw/",
                        $branch,
                        "/",
                        $path
                    ),
                    lang_id_to_string(lang_id, "-", true, "@", false),
                )
            },
        })
    };
}

macro_rules! properties {
    ($id:literal, $name:literal, tor => $branch:literal, $file_name:literal) => {
        Arc::new(DuoProvider {
            id: concat!("torproject-", $id),
            name: $name,
            group_name: Some("The Tor Project"),
            parse: parse_properties,
            default_url: concat!(
                "https://gitlab.torproject.org/tpo/translation/-/raw/",
                $branch,
                "/en-US/",
                $file_name,
                ".properties"
            ),
            url: |lang_id| {
                format!(
                    concat!(
                        "https://gitlab.torproject.org/tpo/translation/-/raw/",
                        $branch,
                        "/{}/",
                        $file_name,
                        ".properties"
                    ),
                    lang_id_to_string(lang_id, "-", true, "@", false),
                )
            },
        })
    };
}

macro_rules! srt {
    ($id:literal, $name:literal, tor => $branch:literal, $default_path:literal, $path:literal) => {
        Arc::new(DuoProvider {
            id: concat!("torproject-", $id),
            name: $name,
            group_name: Some("The Tor Project"),
            parse: parse_srt,
            default_url: concat!(
                "https://gitlab.torproject.org/tpo/translation/-/raw/",
                $branch,
                "/",
                $default_path,
            ),
            url: |lang_id| {
                format!(
                    concat!(
                        "https://gitlab.torproject.org/tpo/translation/-/raw/",
                        $branch,
                        "/",
                        $path,
                    ),
                    lang_id_to_string(lang_id, "-", true, "@", false),
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

        Arc::new(DuoProvider {
            id: "freeshow",
            name: "FreeShow",
            group_name: None,
            parse: parse_freeshow_json,
            default_url: "https://raw.githubusercontent.com/ChurchApps/FreeShow/main/public/lang/en.json",
            url: |lang_id| {
                format!(
                    "https://raw.githubusercontent.com/ChurchApps/FreeShow/main/public/lang/{}.json",
                    lang_id_to_string(lang_id, "_", true, "@", false),
                )
            },
        }),

        po!("duckduckgo", "DuckDuckGo",         None,            None,                  github => "duckduckgo/duckduckgo-locales/master/locales/{}/LC_MESSAGES/duckduckgo.po"),
        po!("multimc",    "MultiMC",            None,            Some('&'), "_", true,  github => "MultiMC/Translations/master/{}.po"),
        po!("weblate",    "Weblate",            Some("Weblate"), None,      "_", false, github => "WeblateOrg/weblate/main/weblate/locale/{}/LC_MESSAGES/django.po"),
        po!("weblatejs",  "Weblate JavaScript", Some("Weblate"), None,      "_", false, github => "WeblateOrg/weblate/main/weblate/locale/{}/LC_MESSAGES/djangojs.po"),

        po!("pacman", "Pacman", None, None,      gitlab => "gitlab.archlinux.org", "pacman/pacman", "master/src/pacman/po/{}.po"),
        po!("wine",   "Wine",   None, Some('&'), gitlab => "gitlab.winehq.org",    "wine/wine",     "master/po/{}.po"),

        android!("etar",                      "Etar",                      github => "Etar-Group/Etar-Calendar"),
        android!("fdroid",                    "F-Droid",                   gitlab => "fdroid/fdroidclient"),
        android!("material-files",            "Material Files",            github => "zhanghai/MaterialFiles"),
        android!("material-files-mime-types", "Material Files mime types", github => "zhanghai/MaterialFiles", "mime_types"),
        android!("notally",                   "Notally",                   github => "OmGodse/Notally"),

        browser_extension!("decentraleyes",       "Decentraleyes",       gitlab => "git.synz.io/Synzvato/decentraleyes", "master/_locales"),
        browser_extension!("improvedtube",        "ImprovedTube",        github => "code-charity/youtube",               "master/_locales"),
        browser_extension!("midnight-lizard",     "Midnight Lizard",     github => "Midnight-Lizard/Midnight-Lizard",    "master/_locales"),
        browser_extension!("simple-translate",    "Simple Translate",    github => "sienori/simple-translate",           "master/src/_locales"),
        browser_extension!("tab-session-manager", "Tab Session Manager", github => "sienori/Tab-Session-Manager",        "master/src/_locales"),
        browser_extension!("tampermonkey",        "Tampermonkey",        github => "Tampermonkey/tampermonkey",          "master/i18n"),
        browser_extension!("tree-style-tab",      "Tree Style Tab",      github => "piroor/treestyletab",                "trunk/webextensions/_locales"),
        browser_extension!("turn-off-the-lights", "Turn Off The Lights", github => "turnoffthelights/Turn-Off-the-Lights-Chrome-extension", "master/src/_locales"),
        browser_extension!("ublock-origin",       "uBlock Origin",       github => "gorhill/uBlock",                     "master/src/_locales"),

        // Tor project

        po!("onion-launchpad",     "Onion Launchpad",     tor => "onion-launchpad",                "contents+{}.po"),
        po!("support-portal",      "Support Portal",      tor => "support-portal",                 "contents+{}.po"),
        po!("torbrowser-manual",   "Tor Browser manual",  tor => "tbmanual-contentspot",           "contents+{}.po"),
        po!("community",           "Tor Community",       tor => "communitytpo-contentspot",       "contents+{}.po"),
        po!("about",               "About Tor Project",   tor => "tpo-web",                        "contents+{}.po"),
        po!("tails-misc",          "Tails miscellaneous", tor => "tails-misc",                     "{}.po"),
        po!("code-of-conduct",     "Code of Conduct",     tor => "policies-code_of_conducttxtpot", "code_of_conduct+{}.po"),
        po!("onion-sprouts-bot",   "Onion Sprouts Bot",   tor => "onionsproutsbot",                "onionsproutsbot+{}.po"),
        po!("tor-animation-title", "Tor Animation title", tor => "tor_animation",                  "title-{}.po"),
        po!("tor-check",           "Tor check",           tor => "torcheck",                       "{}/torcheck.po"),

        dtd!("torbrowser-about-dialog", "Tor Browser about dialog", tor => "tor-browser", "aboutDialog"),
        dtd!("torbrowser-about-update", "Tor Browser about update", tor => "tor-browser", "aboutTBUpdate"),
        dtd!("torbrowser-about-tor",    "Tor Browser about Tor",    tor => "tor-browser", "aboutTor"),
        dtd!("torbrowser-branding",     "Tor Browser branding",     tor => "tor-browser", "brand"),
        dtd!("torbrowser-tor-buttons",  "Tor Browser tor buttons",  tor => "tor-browser", "torbutton"),

        srt!("onionshare-subtitles",    "OnionShare introduction video",  tor => "onionshare-introduction-video-subtitles", "src/onionshare-introduction.srt",  "onionshare-introduction-subs-{}.srt"),
        srt!("bridges-subtitles",       "Bridges introduction video",     tor => "bridges-introduction-video-subtitles",    "src/bridges-introduction.srt",     "bridges-introduction-subtitles-{}.srt"),
        srt!("torbrowser-subtitles",    "Tor Browser introduction video", tor => "tb-introduction-video-subtitles",         "src/tor-browser-introduction.srt", "tor-browser-sub-{}.srt"),
        srt!("tor-animation-subtitles", "Tor animation",                  tor => "tor_animation",                           "Tor_animation.srt",                "subtitles-{}.srt"),

        android!("tor-vpn",        "Tor VPN",         tor => "tor-vpn",                    "res/values/strings.xml",       "res/values-{}/strings.xml"),
        android!("torbrowser-app", "Tor Browser App", tor => "fenix-torbrowserstringsxml", "en-US/torbrowser_strings.xml", "{}/torbrowser_strings.xml"),

        properties!("torbrowser-brand",                "Tor Browser brand",                tor => "tor-browser",  "brand"),
        properties!("torbrowser-browser-onboarding",   "Tor Browser browser onboarding",   tor => "tor-browser",  "browserOnboarding"),
        properties!("torbrowser-crypto-safety-prompt", "Tor Browser crypto safety prompt", tor => "tor-browser",  "cryptoSafetyPrompt"),
        properties!("torbrowser-onboarding",           "Tor Browser onboarding",           tor => "tor-browser",  "onboarding"),
        properties!("torbrowser-onion-location",       "Tor Browser onion location",       tor => "tor-browser",  "onionLocation"),
        properties!("torbrowser-rulesets",             "Tor Browser rulesets",             tor => "tor-browser",  "rulesets"),
        properties!("torbrowser-settings",             "Tor Browser settings",             tor => "tor-browser",  "settings"),
        properties!("torbrowser-tor-connect",          "Tor Browser tor connect",          tor => "tor-browser",  "torConnect"),
        properties!("torbrowser-torbutton",            "Tor Browser torbutton",            tor => "tor-browser",  "torbutton"),
        properties!("torbrowser-torlauncher",          "Tor Browser torlauncher",          tor => "tor-browser",  "torlauncher"),
        properties!("basebrowser-new-identity",        "Base Browser new identity",        tor => "base-browser", "newIdentity"),
        properties!("basebrowser-security-level",      "Base Browser security level",      tor => "base-browser", "securityLevel"),

        browser_extension!("snowflake",         "Snowflake",         tor => "snowflake", "messages"),
        browser_extension!("snowflake-website", "Snowflake website", tor => "snowflake", "website"),

        // Elementary

        po!("appcenter",         "AppCenter",         elementary => "appcenter",   "master/po"),
        po!("appcenter-extra",   "AppCenter Extra",   elementary => "appcenter",   "master/po/extra"),
        po!("calculator",        "Calculator",        elementary => "calculator",  "master/po"),
        po!("calculator-extra",  "Calculator Extra",  elementary => "calculator",  "master/po/extra"),
        po!("calendar",          "Calendar",          elementary => "calendar",    "master/po"),
        po!("calendar-extra",    "Calendar Extra",    elementary => "calendar",    "master/po/extra"),
        po!("camera",            "Camera",            elementary => "camera",      "master/po"),
        po!("camera-extra",      "Camera Extra",      elementary => "camera",      "master/po/extra"),
        po!("code",              "Code",              elementary => "code",        "master/po"),
        po!("code-extra",        "Code Extra",        elementary => "code",        "master/po/extra"),
        po!("code-plugins",      "Code Plugins",      elementary => "code",        "master/po/plugins"),
        po!("files",             "Files",             elementary => "files",       "main/po"),
        po!("files-extra",       "Files Extra",       elementary => "files",       "main/po/extra"),
        po!("friends",           "Friends",           elementary => "friends",     "master/po"),
        po!("friends-extra",     "Friends Extra",     elementary => "friends",     "master/po/extra"),
        po!("installer",         "Installer",         elementary => "installer",   "master/po"),
        po!("installer-extra",   "Installer Extra",   elementary => "installer",   "master/po/extra"),
        po!("mail",              "Mail",              elementary => "mail",        "master/po"),
        po!("mail-extra",        "Mail Extra",        elementary => "mail",        "master/po/extra"),
        po!("music",             "Music",             elementary => "music",       "main/po"),
        po!("music-extra",       "Music Extra",       elementary => "music",       "main/po/extra"),
        po!("photos",            "Photos",            elementary => "photos",      "master/po"),
        po!("photos-extra",      "Photos Extra",      elementary => "photos",      "master/po/extra"),
        po!("screenshot",        "Screenshot",        elementary => "screenshot",  "master/po"),
        po!("screenshot-extra",  "Screenshot Extra",  elementary => "screenshot",  "master/po/extra"),
        po!("switchboard",       "Switchboard",       elementary => "switchboard", "main/po"),
        po!("switchboard-extra", "Switchboard Extra", elementary => "switchboard", "main/po/extra"),
        po!("tasks",             "Tasks",             elementary => "tasks",       "master/po"),
        po!("tasks-extra",       "Tasks Extra",       elementary => "tasks",       "master/po/extra"),
        po!("terminal",          "Terminal",          elementary => "terminal",    "master/po"),
        po!("terminal-extra",    "Terminal Extra",    elementary => "terminal",    "master/po/extra"),
        po!("videos",            "Videos",            elementary => "videos",      "main/po"),
        po!("videos-extra",      "Videos Extra",      elementary => "videos",      "main/po/extra"),
        po!("wingpanel",         "Wingpanel",         elementary => "wingpanel",   "master/po"),
        po!("wingpanel-extra",   "Wingpanel Extra",   elementary => "wingpanel",   "master/po/extra"),

        po!("capnet-assist",               "Captive Network Assistant",          elementary => "capnet-assist",         "master/po"),
        po!("capnet-assist-extra",         "Captive Network Assistant Extra",    elementary => "capnet-assist",         "master/po/extra"),
        po!("feedback",                    "Feedback",                           elementary => "feedback",              "master/po"),
        po!("feedback-extra",              "Feedback Extra",                     elementary => "feedback",              "master/po/extra"),
        po!("flatpak-platform",            "Flatpak Platform",                   elementary => "flatpak-platform",      "main/platform-data/po"),
        po!("gala",                        "Gala",                               elementary => "gala",                  "master/po"),
        po!("granite",                     "Granite",                            elementary => "granite",               "main/po"),
        po!("granite-extra",               "Granite Extra",                      elementary => "granite",               "main/po/extra"),
        po!("greeter",                     "Greeter",                            elementary => "greeter",               "master/po"),
        po!("greeter-extra",               "Greeter Extra",                      elementary => "greeter",               "master/po/extra"),
        po!("icons",                       "Icons",                              elementary => "icons",                 "main/po"),
        po!("notifications",               "Notifications",                      elementary => "notifications",         "master/po/extra"),
        po!("pantheon-agent-polkit",       "Pantheon Polkit Agent",              elementary => "pantheon-agent-polkit", "main/po"),
        po!("pantheon-agent-polkit-extra", "Pantheon Polkit Agent Extra",        elementary => "pantheon-agent-polkit", "main/po/extra"),
        po!("portals",                     "Pantheon XDG Desktop Portals",       elementary => "portals",               "main/po"),
        po!("portals-extra",               "Pantheon XDG Desktop Portals Extra", elementary => "portals",               "main/po/extra"),
        po!("settings-daemon",             "Settings Daemon",                    elementary => "settings-daemon",       "master/po"),
        po!("shortcut-overlay",            "Shortcut Overlay",                   elementary => "shortcut-overlay",      "master/po"),
        po!("shortcut-overlay-extra",      "Shortcut Overlay Extra",             elementary => "shortcut-overlay",      "master/po/extra"),
        po!("sideload",                    "Sideload",                           elementary => "sideload",              "master/po"),
        po!("sideload-extra",              "Sideload Extra",                     elementary => "sideload",              "master/po/extra"),
        po!("stylesheet",                  "Stylesheet",                         elementary => "stylesheet",            "master/po"),
        po!("wallpapers",                  "Wallpapers",                         elementary => "wallpapers",            "master/po"),

        json!("website-docs-installation",        elementary => "docs/installation"),
        json!("website-docs-learning-the-basics", elementary => "docs/learning-the-basics"),
        json!("website-docs-translation-guide",   elementary => "docs/translation-guide"),
        json!("website-store-index",              elementary => "store/index"),
        json!("website-store-cart",               elementary => "store/cart"),
        json!("website-403",                      elementary => "403"),
        json!("website-404",                      elementary => "404"),
        json!("website-410",                      elementary => "410"),
        json!("website-security",                 elementary => "SECURITY"),
        json!("website-brand",                    elementary => "brand"),
        json!("website-capnet-assist",            elementary => "capnet-assist"),
        json!("website-code-of-conduct",          elementary => "code-of-conduct"),
        json!("website-get-involved",             elementary => "get-involved"),
        json!("website-index",                    elementary => "index"),
        json!("website-layout",                   elementary => "layout"),
        json!("website-oem",                      elementary => "oem"),
        json!("website-open-source",              elementary => "open-source"),
        json!("website-press",                    elementary => "press"),
        json!("website-privacy",                  elementary => "privacy"),
        json!("website-support",                  elementary => "support"),
        json!("website-thank-you",                elementary => "thank-you"),

        // Android source

        android!(source => "bootable/recovery",                         "tools/recovery_l10n/res"),
        android!(source => "development",                               "apps/Fallback/res"),
        android!(source => "frameworks/base",                           "core/res/res"),
        android!(source => "frameworks/base",                           "libs/WindowManager/Shell/res"),
        android!(source => "frameworks/base",                           "packages/BackupRestoreConfirmation/res"),
        android!(source => "frameworks/base",                           "packages/CarrierDefaultApp/res"),
        android!(source => "frameworks/base",                           "packages/CompanionDeviceManager/res"),
        android!(source => "frameworks/base",                           "packages/DynamicSystemInstallationService/res"),
        android!(source => "frameworks/base",                           "packages/ExternalStorageProvider/res"),
        android!(source => "frameworks/base",                           "packages/FusedLocation/res"),
        android!(source => "frameworks/base",                           "packages/InputDevices/res"),
        android!(source => "frameworks/base",                           "packages/PackageInstaller/res"),
        android!(source => "frameworks/base",                           "packages/PrintSpooler/res"),
        android!(source => "frameworks/base",                           "packages/SettingsLib/BannerMessagePreference/res"),
        android!(source => "frameworks/base",                           "packages/SettingsLib/FooterPreference/res"),
        android!(source => "frameworks/base",                           "packages/SettingsLib/HelpUtils/res"),
        android!(source => "frameworks/base",                           "packages/SettingsLib/RestrictedLockUtils/res"),
        android!(source => "frameworks/base",                           "packages/SettingsLib/SearchWidget/res"),
        android!(source => "frameworks/base",                           "packages/SettingsLib/SelectorWithWidgetPreference/res"),
        android!(source => "frameworks/base",                           "packages/SettingsLib/res"),
        android!(source => "frameworks/base",                           "packages/SettingsProvider/res"),
        android!(source => "frameworks/base",                           "packages/Shell/res"),
        android!(source => "frameworks/base",                           "packages/SimAppDialog/res"),
        android!(source => "frameworks/base",                           "packages/SoundPicker/res"),
        android!(source => "frameworks/base",                           "packages/SystemUI/res-keyguard"),
        android!(source => "frameworks/base",                           "packages/SystemUI/res-product"),
        android!(source => "frameworks/base",                           "packages/SystemUI/res"),
        android!(source => "frameworks/base",                           "packages/VpnDialogs/res"),
        android!(source => "frameworks/base",                           "packages/WallpaperCropper/res"),
        android!(source => "frameworks/base",                           "packages/overlays/AvoidAppsInCutoutOverlay/res"),
        android!(source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationCornerOverlay/res"),
        android!(source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationDoubleOverlay/res"),
        android!(source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationHoleOverlay/res"),
        android!(source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationNarrowOverlay/res"),
        android!(source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationTallOverlay/res"),
        android!(source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationWaterfallOverlay/res"),
        android!(source => "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationWideOverlay/res"),
        android!(source => "frameworks/base",                           "packages/overlays/NoCutoutOverlay/res"),
        android!(source => "frameworks/opt/chips",                      "res"),
        android!(source => "frameworks/opt/chips",                      "sample/res"),
        android!(source => "frameworks/opt/colorpicker",                "res"),
        android!(source => "frameworks/opt/net/wifi",                   "libs/WifiTrackerLib/res"),
        android!(source => "frameworks/opt/photoviewer",                "res"),
        android!(source => "frameworks/opt/photoviewer",                "sample/res"),
        android!(source => "frameworks/opt/setupwizard",                "library/main/res"),
        android!(source => "frameworks/opt/timezonepicker",             "res"),
        android!(source => "packages/apps/BasicSmsReceiver",            "res"),
        android!(source => "packages/apps/Calendar",                    "res"),
        android!(source => "packages/apps/Camera2",                     "res"),
        android!(source => "packages/apps/Car/Calendar",                "res"),
        android!(source => "packages/apps/Car/Launcher",                "res"),
        android!(source => "packages/apps/Car/LinkViewer",              "res"),
        android!(source => "packages/apps/Car/Notification",            "res"),
        android!(source => "packages/apps/Car/Settings",                "res"),
        android!(source => "packages/apps/Car/SystemUI",                "res"),
        android!(source => "packages/apps/Car/SystemUpdater",           "res"),
        android!(source => "packages/apps/Car/systemlibs",              "car-assist-client-lib/res"),
        android!(source => "packages/apps/Car/systemlibs",              "car-broadcastradio-support/res"),
        android!(source => "packages/apps/CellBroadcastReceiver",       "res"),
        android!(source => "packages/apps/CertInstaller",               "res"),
        android!(source => "packages/apps/Contacts",                    "res"),
        android!(source => "packages/apps/DeskClock",                   "res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/about/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/app/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/assisteddialing/ui/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/blocking/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/blockreportspam/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/callcomposer/cameraui/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/callcomposer/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/calldetails/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/calllog/ui/menu/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/calllog/ui/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/calllogutils/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/clipboard/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/common/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/contactphoto/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/dialpadview/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/glidephotomanager/impl/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/historyitemactions/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/interactions/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/main/impl/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/notification/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/phonenumberutil/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/postcall/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/precall/impl/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/preferredsim/impl/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/preferredsim/suggestion/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/promotion/impl/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/cp2/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/directories/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/list/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/nearbyplaces/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/shortcuts/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/spam/promo/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/spannable/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/speeddial/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/theme/common/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/util/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/voicemail/listui/error/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/voicemail/settings/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/dialer/widget/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/answer/impl/answermethod/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/answer/impl/hint/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/answer/impl/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/audioroute/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/commontheme/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/contactgrid/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/disconnectdialog/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/hold/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/incall/impl/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/rtt/impl/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/sessiondata/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/spam/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/telecomeventui/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/theme/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/incallui/video/impl/res"),
        android!(source => "packages/apps/Dialer",                      "java/com/android/voicemail/impl/res"),
        android!(source => "packages/apps/DocumentsUI",                 "res"),
        android!(source => "packages/apps/EmergencyInfo",               "EmergencyGestureAction/res"),
        android!(source => "packages/apps/EmergencyInfo",               "res"),
        android!(source => "packages/apps/Gallery",                     "res"),
        android!(source => "packages/apps/Gallery2",                    "res"),
        android!(source => "packages/apps/HTMLViewer",                  "res"),
        android!(source => "packages/apps/KeyChain",                    "res"),
        android!(source => "packages/apps/Launcher3",                   "go/quickstep/res"),
        android!(source => "packages/apps/Launcher3",                   "quickstep/res"),
        android!(source => "packages/apps/Launcher3",                   "res"),
        android!(source => "packages/apps/LegacyCamera",                "res"),
        android!(source => "packages/apps/ManagedProvisioning",         "res"),
        android!(source => "packages/apps/Messaging",                   "res"),
        android!(source => "packages/apps/Music",                       "kotlin/res"),
        android!(source => "packages/apps/MusicFX",                     "res"),
        android!(source => "packages/apps/Nfc",                         "res"),
        android!(source => "packages/apps/PhoneCommon",                 "res"),
        android!(source => "packages/apps/Protips",                     "res"),
        android!(source => "packages/apps/QuickAccessWallet",           "res"),
        android!(source => "packages/apps/SafetyRegulatoryInfo",        "res"),
        android!(source => "packages/apps/Settings",                    "res"),
        android!(source => "packages/apps/SettingsIntelligence",        "res"),
        android!(source => "packages/apps/Stk",                         "res"),
        android!(source => "packages/apps/StorageManager",              "res"),
        android!(source => "packages/apps/TV",                          "common/res"),
        android!(source => "packages/apps/TV",                          "res"),
        android!(source => "packages/apps/Tag",                         "res"),
        android!(source => "packages/apps/ThemePicker",                 "res"),
        android!(source => "packages/apps/Traceur",                     "res"),
        android!(source => "packages/apps/TvSettings",                  "Settings/res-twopanel"),
        android!(source => "packages/apps/TvSettings",                  "Settings/res"),
        android!(source => "packages/apps/TvSettings",                  "TwoPanelSettingsLib/res"),
        android!(source => "packages/apps/WallpaperPicker",             "res"),
        android!(source => "packages/apps/WallpaperPicker2",            "res"),
        android!(source => "packages/inputmethods/LatinIME",            "java/res"),
        android!(source => "packages/inputmethods/LeanbackIME",         "res"),
        android!(source => "packages/modules/Bluetooth",                "android/app/res"),
        android!(source => "packages/modules/CaptivePortalLogin",       "res"),
        android!(source => "packages/modules/CellBroadcastService",     "res"),
        android!(source => "packages/modules/Connectivity",             "Tethering/res"),
        android!(source => "packages/modules/Connectivity",             "service/ServiceConnectivityResources/res"),
        android!(source => "packages/modules/ExtServices",              "java/res"),
        android!(source => "packages/modules/NetworkStack",             "res"),
        android!(source => "packages/modules/Permission",               "PermissionController/res"),
        android!(source => "packages/modules/Permission",               "SafetyCenter/Resources/res"),
        android!(source => "packages/modules/Wifi",                     "OsuLogin/res"),
        android!(source => "packages/modules/Wifi",                     "service/ServiceWifiResources/res"),
        android!(source => "packages/providers/BlockedNumberProvider",  "res"),
        android!(source => "packages/providers/CalendarProvider",       "res"),
        android!(source => "packages/providers/ContactsProvider",       "res"),
        android!(source => "packages/providers/DownloadProvider",       "res"),
        android!(source => "packages/providers/DownloadProvider",       "ui/res"),
        android!(source => "packages/providers/MediaProvider",          "res"),
        android!(source => "packages/providers/TelephonyProvider",      "res"),
        android!(source => "packages/providers/TvProvider",             "res"),
        android!(source => "packages/providers/UserDictionaryProvider", "res"),
        android!(source => "packages/screensavers/Basic",               "res"),
        android!(source => "packages/screensavers/PhotoTable",          "res"),
        android!(source => "packages/services/BuiltInPrintService",     "res"),
        android!(source => "packages/services/Car",                     "FrameworkPackageStubs/res"),
        android!(source => "packages/services/Car",                     "car-admin-ui-lib/src/main/res"),
        android!(source => "packages/services/Car",                     "car-maps-placeholder/res"),
        android!(source => "packages/services/Car",                     "car-usb-handler/res"),
        android!(source => "packages/services/Car",                     "car_product/car_ui_portrait/apps/CarUiPortraitSystemUI/res"),
        // The commented translations has no default translation
        // android!("", source => "packages/services/Car",                  "car_product/car_ui_portrait/rro/CarEvsCameraPreviewAppRRO/res"),
        // android!("", source => "packages/services/Car",                  "car_product/car_ui_portrait/rro/CarUiPortraitDialerRRO/res"),
        // android!("", source => "packages/services/Car",                  "car_product/car_ui_portrait/rro/CarUiPortraitNotificationRRO/res"),
        android!(source => "packages/services/Car",                     "car_product/overlay/frameworks/base/core/res/res"),
        android!(source => "packages/services/Car",                     "experimental/service/res"),
        android!(source => "packages/services/Car",                     "packages/CarDeveloperOptions/res"),
        android!(source => "packages/services/Car",                     "packages/CarManagedProvisioning/res"),
        android!(source => "packages/services/Car",                     "service-builtin/res"),
        android!(source => "packages/services/Car",                     "service/res"),
        android!(source => "packages/services/Car",                     "tests/BugReportApp/res"),
        android!(source => "packages/services/Car",                     "tests/DiagnosticTools/res"),
        android!(source => "packages/services/Car",                     "tests/MultiDisplaySecondaryHomeTestLauncher/res"),
        android!(source => "packages/services/Car",                     "tests/MultiDisplayTest/res"),
        android!(source => "packages/services/Car",                     "tests/MultiDisplayTestHelloActivity/res"),
        android!(source => "packages/services/Mtp",                     "res"),
        android!(source => "packages/services/Telecomm",                "res"),
        android!(source => "packages/services/Telephony",               "res"),
        android!(source => "packages/services/Telephony",               "testapps/GbaTestApp/res"),
        android!(source => "packages/services/Telephony",               "testapps/TestSliceApp/app/src/main/res"),
        android!(source => "packages/wallpapers/LivePicker",            "res"),
    ];

    #[cfg(debug_assertions)]
    {
        use std::collections::HashSet;

        let mut set = HashSet::with_capacity(providers.len());
        for (i, provider) in providers.iter().enumerate() {
            if !provider.id().starts_with("android-") && !provider.id().chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
                panic!("Invalid id: '{}', at index {i}", provider.id());
            }
            if provider.group_name() != Some("Android") && provider.name() == "" {
                panic!("Provider has empty name: '{}', at index {i}", provider.id());
            }

            if set.contains(provider.id()) {
                panic!("Duplicate id: '{}', second at index {i}", provider.id());
            }
            set.insert(provider.id());

            if !provider.name().is_empty() && set.contains(provider.name()) {
                panic!("Duplicate name: '{}', second at index {i}", provider.name());
            }
            set.insert(provider.name());
        }
    }

    providers
}
