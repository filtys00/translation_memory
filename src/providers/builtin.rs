// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::vec;

use super::{
    android::{android_urls, parse_android, parse_android_base64},
    browser_extension::{parse_browser_extension, parse_dark_reader},
    chrome::{chromium_urls, parse_xtb_base64},
    dtd::parse_dtd,
    eu::{get_eu_source, parse_eu_tmx},
    gnome::graphql_gnome,
    json::{parse_elementary_json, parse_geogebra_js_json, parse_json},
    kde::graphql_kde,
    libreoffice::crawl_libreoffice,
    minecraft::{get_minecraft_sources, parse_minecraft},
    mozilla::{parse_mozilla_tbx, parse_mozilla_tmx},
    po::{parse_po, parse_po_base64_remove_underscore, parse_po_remove_ampersand, parse_po_remove_underscore},
    properties::{parse_obs_studio_ini, parse_properties},
    srt::parse_srt,
    ts::parse_qbittorrent_ts,
    yaml::parse_mastodon_yaml,
    Provider,
};

macro_rules! mono {
    ($id:expr, $name:expr, $group_name:expr, $parse:ident, url => {
        url: $url:expr,
        lang_id: $region_binder:literal, $uppercase_region:literal, $variant_binder:literal, $uppercase_variant:literal,
    }) => {
        Provider::new_mono_one_per_lang($id, $name, $group_name, $parse, |lang_id| {
            format!($url, lang_id.format($region_binder, $uppercase_region, $variant_binder, $uppercase_variant))
        })
    };
    ($id:expr, $name:expr, $group_name:expr, $parse:ident, github => {
        url: $url:expr,
        lang_id: $region_binder:literal, $uppercase_region:literal, $variant_binder:literal, $uppercase_variant:literal,
    }) => {
        mono!($id, $name, $group_name, $parse, url => {
            url: concat!("https://raw.githubusercontent.com/", $url),
            lang_id: $region_binder, $uppercase_region, $variant_binder, $uppercase_variant,
        })
    };
    ($id:expr, $name:expr, $group_name:expr, $parse:ident, gitlab => {
        base_url: $base_url:expr,
        url: $url:expr,
        lang_id: $region_binder:literal, $uppercase_region:literal, $variant_binder:literal, $uppercase_variant:literal,
    }) => {
        mono!($id, $name, $group_name, $parse, url => {
            url: concat!("https://", $base_url, "/-/raw/", $url),
            lang_id: $region_binder, $uppercase_region, $variant_binder, $uppercase_variant,
        })
    };
}

macro_rules! duo {
    ($id:expr, $name:expr, $group_name:expr, $parse:ident, url => {
        default_url: $default_url:expr,
        url: $url:expr,
        lang_id: $region_binder:literal, $uppercase_region:literal, $variant_binder:literal, $uppercase_variant:literal,
    }) => {
        Provider::new_duo_one_per_lang($id, $name, $group_name, $parse, $default_url, |lang_id| {
            format!($url, lang_id.format($region_binder, $uppercase_region, $variant_binder, $uppercase_variant))
        })
    };
    ($id:expr, $name:expr, $group_name:expr, $parse:ident, github => {
        default_url: $default_url:expr,
        url: $url:expr,
        lang_id: $region_binder:literal, $uppercase_region:literal, $variant_binder:literal, $uppercase_variant:literal,
    }) => {
        duo!($id, $name, $group_name, $parse, url => {
            default_url: concat!("https://raw.githubusercontent.com/", $default_url),
            url: concat!("https://raw.githubusercontent.com/", $url),
            lang_id: $region_binder, $uppercase_region, $variant_binder, $uppercase_variant,
        })
    };
    ($id:expr, $name:expr, $group_name:expr, $parse:ident, gitlab => {
        base_url: $base_url:expr,
        default_url: $default_url:expr,
        url: $url:expr,
        lang_id: $region_binder:literal, $uppercase_region:literal, $variant_binder:literal, $uppercase_variant:literal,
    }) => {
        duo!($id, $name, $group_name, $parse, url => {
            default_url: concat!("https://", $base_url, "/-/raw/", $default_url),
            url: concat!("https://", $base_url, "/-/raw/", $url),
            lang_id: $region_binder, $uppercase_region, $variant_binder, $uppercase_variant,
        })
    };
}

macro_rules! android {
    ($id:literal, $name:literal, github => $repo:literal, $file_name:literal) => {
        duo!($id, $name, Some("Android apps"), parse_android, github => {
            default_url: concat!($repo, "/HEAD/app/src/main/res/values/",    $file_name),
                    url: concat!($repo, "/HEAD/app/src/main/res/values-{}/", $file_name),
            lang_id: "-r", true, "-", false,
        })
    };
    ($id:literal, $name:literal, gitlab => $repo:literal, $file_name:literal) => {
        duo!($id, $name, Some("Android apps"), parse_android, gitlab => {
            base_url: concat!("gitlab.com/", $repo),
            default_url: concat!("HEAD/app/src/main/res/values/",    $file_name),
                    url: concat!("HEAD/app/src/main/res/values-{}/", $file_name),
            lang_id: "-r", true, "-", false,
        })
    };
}

macro_rules! browser_extension {
    ($id:literal, $name:literal, github => $repo:literal, $folder:literal, $default_lang_id:literal) => {
        duo!($id, $name, Some("Browser extensions"), parse_browser_extension, github => {
            default_url: concat!($repo, "/HEAD/", $folder, "/", $default_lang_id, "/messages.json"),
                    url: concat!($repo, "/HEAD/", $folder, "/{}/messages.json"),
            lang_id: "_", true, "_", false,
        })
    };
    ($id:literal, $name:literal, gitlab => $base_url:literal, $folder:literal, $default_lang_id:literal) => {
        duo!($id, $name, Some("Browser extensions"), parse_browser_extension, gitlab => {
            base_url: $base_url,
            default_url: concat!("HEAD/", $folder, "/", $default_lang_id, "/messages.json"),
                    url: concat!("HEAD/", $folder, "/{}/messages.json"),
            lang_id: "_", true, "_", false,
        })
    };
}

macro_rules! elementary {
    ($id:literal, json => $path:literal) => {
        mono!(concat!("elementary-", $id), concat!("Elementary ", $path), Some("Elementary"), parse_elementary_json, github => {
            url: concat!("elementary/website/master/_lang/{}/", $path, ".json"),
            lang_id: "_", true, "@", false,
        })
    };
    ($id:literal, $name:literal, po => $repo:literal, $path:literal) => {
        mono!(concat!("elementary-", $id), $name, Some("Elementary"), parse_po_remove_underscore, github => {
            url: concat!("elementary/", $repo, "/", $path, "/{}.po"),
            lang_id: "_", true, "_", false,
        })
    };
}

macro_rules! geogebra {
    ($id:literal, $name:literal, properties => $file_base:literal) => {
        duo!($id, $name, Some("GeoGebra"), parse_properties, github => {
            default_url: concat!("geogebra/geogebra/master/common-jre/src/nonfree/resources/org/geogebra/common/jre/properties/", $file_base, ".properties"),
                    url: concat!("geogebra/geogebra/master/common-jre/src/nonfree/resources/org/geogebra/common/jre/properties/", $file_base, "_{}.properties"),
            lang_id: "_", true, "_", false,
        })
    };
}

macro_rules! mastodon {
    ($id:literal, $name:literal, yaml => $default_file_name:literal, $file_name:literal) => {
        duo!($id, $name, Some("Mastodon"), parse_mastodon_yaml, github => {
            default_url: concat!("mastodon/mastodon/main/config/locales/", $default_file_name),
                    url: concat!("mastodon/mastodon/main/config/locales/", $file_name),
            lang_id: "-", true, "-", false,
        })
    };
}

macro_rules! tor {
    ($id:literal, $name:literal, android => $branch:literal, $default_path:literal, $path:literal) => {
        duo!(concat!("torproject-", $id), $name, Some("The Tor Project"), parse_android, gitlab => {
            base_url: "gitlab.torproject.org/tpo/translation",
            default_url: concat!($branch, "/", $default_path),
                    url: concat!($branch, "/", $path),
            lang_id: "-r", true, "-", false,
        })
    };
    ($id:literal, $name:literal, browser_extension => $branch:literal, $file_name:literal) => {
        duo!(concat!("torproject-", $id), $name, Some("The Tor Project"), parse_browser_extension, gitlab => {
            base_url: "gitlab.torproject.org/tpo/translation",
            default_url: concat!($branch, "/en_US/", $file_name),
                    url: concat!($branch, "/{}/",    $file_name),
            lang_id: "_", true, "_", false,
        })
    };
    ($id:literal, $name:literal, dtd => $branch:literal, $file_name:literal) => {
        duo!(concat!("torproject-", $id), $name, Some("The Tor Project"), parse_dtd, gitlab => {
            base_url: "gitlab.torproject.org/tpo/translation",
            default_url: concat!($branch, "/en-US/", $file_name),
                    url: concat!($branch, "/{}/",    $file_name),
            lang_id: "-", true, "@", false,
        })
    };
    ($id:literal, $name:literal, properties => $branch:literal, $file_name:literal) => {
        duo!(concat!("torproject-", $id), $name, Some("The Tor Project"), parse_properties, gitlab => {
            base_url: "gitlab.torproject.org/tpo/translation",
            default_url: concat!($branch, "/en-US/", $file_name),
                    url: concat!($branch, "/{}/",    $file_name),
            lang_id: "-", true, "@", false,
        })
    };
    ($id:literal, $name:literal, po => $branch:literal, $path:literal) => {
        mono!(concat!("torproject-", $id), $name, Some("The Tor Project"), parse_po, gitlab => {
            base_url: "gitlab.torproject.org/tpo/translation",
            url: concat!($branch, "/", $path),
            lang_id: "-", true, "@", false,
        })
    };
    ($id:literal, $name:literal, srt => $branch:literal, $default_path:literal, $path:literal) => {
        duo!(concat!("torproject-", $id), $name, Some("The Tor Project"), parse_srt, gitlab => {
            base_url: "gitlab.torproject.org/tpo/translation",
            default_url: concat!($branch, "/", $default_path),
                    url: concat!($branch, "/", $path),
            lang_id: "-", true, "@", false,
        })
    };
}

#[rustfmt::skip]
pub fn builtin_providers() -> Vec<Provider<'static>> {
    let providers: Vec<Provider> = vec![
        Provider::new("eu", "European Commision", None, parse_eu_tmx, get_eu_source),
        Provider::new("minecraft", "Minecraft", None, parse_minecraft, get_minecraft_sources),
        Provider::new_mono_one_per_lang("mozilla-terminology", "Mozilla terminology", Some("Mozilla"), parse_mozilla_tbx,
            |lang_id| format!("https://pontoon.mozilla.org/terminology/{}.tbx", lang_id.format("-", true, "-", false))
        ),
        Provider::new_mono_one_per_lang("mozilla", "Mozilla", Some("Mozilla"), parse_mozilla_tmx,
            |lang_id| format!("https://pontoon.mozilla.org/translation-memory/{}.all-projects.tmx", lang_id.format("-", true, "-", false))
        ),
        Provider::new_duo_many_per_langs("android", "Android", None, parse_android_base64, android_urls),
        Provider::new_duo_many_per_langs("chrome", "Chromium", None, parse_xtb_base64, chromium_urls),
        Provider::new_mono_many_per_lang("gnome", "GNOME", None, parse_po_remove_underscore, graphql_gnome),
        Provider::new_mono_many_per_lang("kde", "KDE", None, parse_po_remove_ampersand, graphql_kde),
        Provider::new_mono_many_per_lang("libreoffice", "LibreOffice", None, parse_po_base64_remove_underscore, crawl_libreoffice),

        mono!("arduino",     "Arduino IDE",        None,                       parse_po,           github => {
            url: "arduino/Arduino/master/arduino-core/src/processing/app/i18n/Resources_{}.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("audacity",    "Audacity",           None,                       parse_po,           github => {
            url: "audacity/audacity/master/locale/{}.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("bottles",     "Bottles",            None,                       parse_po,           github => {
            url: "bottlesdevs/Bottles/main/po/{}.po",
            lang_id: "_", true, "@", false,
        }),
        duo!( "dark-reader", "Dark Reader",        Some("Browser extensions"), parse_dark_reader,  github => {
            default_url: "darkreader/darkreader/main/src/_locales/en.config",
                    url: "darkreader/darkreader/main/src/_locales/{}.config",
            lang_id: "-", true, "@", false,
        }),
        mono!("darktable",   "Darktable",          None,                       parse_po,           github => {
            url: "darktable-org/darktable/master/po/{}.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("dolphin-emulator", "Dolphin",       None,                       parse_po,           github => {
            url: "dolphin-emu/dolphin/master/Languages/po/{}.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("duckduckgo",  "DuckDuckGo",         None,                       parse_po,           github => {
            url: "duckduckgo/duckduckgo-locales/master/locales/{}/LC_MESSAGES/duckduckgo.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("extension-manager", "Extension Manager", None,                  parse_po,           github => {
            url: "mjakeman/extension-manager/master/po/{}.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("flatseal",    "Flatseal",           None,                       parse_po,           github => {
            url: "tchx84/Flatseal/master/po/{}.po",
            lang_id: "_", true, "@", false,
        }),
        duo!( "freeshow",    "FreeShow",           None,                       parse_json,         github => {
            default_url: "ChurchApps/FreeShow/main/public/lang/en.json",
                    url: "ChurchApps/FreeShow/main/public/lang/{}.json",
            lang_id: "_", true, "@", false,
        }),
        mono!("inkscape",    "Inkscape",           Some("Inkscape"),           parse_po_remove_underscore, gitlab => {
            base_url: "gitlab.com/inkscape/inkscape",
            url: "master/po/{}.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("multimc",     "MultiMC",            None,                       parse_po_remove_ampersand, github => {
            url: "MultiMC/Translations/master/{}.po",
            lang_id: "_", true, "_", true,
        }),
        mono!("lyx",         "LyX",                None,                       parse_po_remove_ampersand, url => {
            url: "https://git.lyx.org/gitweb/?p=lyx.git;a=blob_plain;f=po/{}.po;hb=HEAD",
            lang_id: "_", true, "@", true,
        }),
        duo!( "obs-studio",  "OBS Studio",         None,                       parse_obs_studio_ini, github => {
            default_url: "obsproject/obs-studio/master/UI/data/locale/en-US.ini",
                    url: "obsproject/obs-studio/master/UI/data/locale/{}.ini",
            lang_id: "-", true, "-", false,
        }),
        duo!( "obsidian",    "Obsidian",           None,                       parse_json,         github => {
            default_url: "obsidianmd/obsidian-translations/master/en.json",
                    url: "obsidianmd/obsidian-translations/master/{}.json",
            lang_id: "-", true, "-", false,
        }),
        mono!("pacman",      "Pacman",             None,                       parse_po,           gitlab => {
            base_url: "gitlab.archlinux.org/pacman/pacman",
            url: "master/src/pacman/po/{}.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("poedit",      "Poedit",             None,                       parse_po,           github => {
            url: "vslavik/poedit/master/locales/{}.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("qbittorrent", "qBittorrent",        None,                       parse_qbittorrent_ts, github => {
            url: "qbittorrent/qBittorrent/master/src/lang/qbittorrent_{}.ts",
            lang_id: "_", true, "@", false,
        }),
        mono!("strawberry",  "Strawberry",         None,                       parse_po_remove_ampersand, github => {
            url: "strawberrymusicplayer/strawberry/master/src/translations/{}.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("trac",        "Trac",               Some("Trac"),               parse_po,           url => {
            url: "https://trac.edgewall.org/browser/trunk/trac/locale/{}/LC_MESSAGES/messages.po?format=txt",
            lang_id: "_", true, "@", false,
        }),
        mono!("trac-js",     "Trac JavaScript",    Some("Trac"),               parse_po,           url => {
            url: "https://trac.edgewall.org/browser/trunk/trac/locale/{}/LC_MESSAGES/messages-js.po?format=txt",
            lang_id: "_", true, "@", false,
        }),
        mono!("trac-ini",    "Trac INI",           Some("Trac"),               parse_po,           url => {
            url: "https://trac.edgewall.org/browser/trunk/trac/locale/{}/LC_MESSAGES/tracini.po?format=txt",
            lang_id: "_", true, "@", false,
        }),
        mono!("vlc",         "VLC",                None,                       parse_po,           github => {
            url: "videolan/vlc/master/po/{}.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("weblate",     "Weblate",            Some("Weblate"),            parse_po,           github => {
            url: "WeblateOrg/weblate/main/weblate/locale/{}/LC_MESSAGES/django.po",
            lang_id: "_", true, "_", false,
        }),
        mono!("weblatejs",   "Weblate JavaScript", Some("Weblate"),            parse_po,           github => {
            url: "WeblateOrg/weblate/main/weblate/locale/{}/LC_MESSAGES/djangojs.po",
            lang_id: "_", true, "_", false,
        }),
        mono!("wine",        "Wine",               None,                       parse_po_remove_ampersand, gitlab => {
            base_url: "gitlab.winehq.org/wine/wine",
            url: "master/po/{}.po",
            lang_id: "_", true, "@", false,
        }),
        mono!("xdg-shared-mime-info", "XDG shared mime info", None, parse_po, gitlab => {
            base_url: "gitlab.freedesktop.org/xdg/shared-mime-info",
            url: "master/po/{}.po",
            lang_id: "_", true, "@", false,
        }),

        android!("davx5",                     "DAVx⁵",                     github => "bitfireAT/davx5-ose",      "strings.xml"),
        android!("etar",                      "Etar",                      github => "Etar-Group/Etar-Calendar", "strings.xml"),
        android!("fdroid",                    "F-Droid",                   gitlab => "fdroid/fdroidclient",      "strings.xml"),
        android!("material-files",            "Material Files",            github => "zhanghai/MaterialFiles",   "strings.xml"),
        android!("material-files-mime-types", "Material Files mime types", github => "zhanghai/MaterialFiles",   "mime_types.xml"),
        android!("notally",                   "Notally",                   github => "OmGodse/Notally",          "strings.xml"),

        browser_extension!("decentraleyes",       "Decentraleyes",       gitlab => "git.synz.io/Synzvato/decentraleyes", "_locales",                        "en_US"),
        browser_extension!("improvedtube",        "ImprovedTube",        github => "code-charity/youtube",               "_locales",                        "en"),
        browser_extension!("midnight-lizard",     "Midnight Lizard",     github => "Midnight-Lizard/Midnight-Lizard",    "_locales",                        "en"),
        browser_extension!("simple-translate",    "Simple Translate",    github => "sienori/simple-translate",           "src/_locales",                    "en"),
        browser_extension!("tab-session-manager", "Tab Session Manager", github => "sienori/Tab-Session-Manager",        "src/_locales",                    "en"),
        browser_extension!("tampermonkey",        "Tampermonkey",        github => "Tampermonkey/tampermonkey",          "i18n",                            "en"),
        browser_extension!("tree-style-tab",      "Tree Style Tab",      github => "piroor/treestyletab",                "webextensions/_locales",          "en"),
        browser_extension!("turn-off-the-lights", "Turn Off The Lights", github => "turnoffthelights/Turn-Off-the-Lights-Chrome-extension", "src/_locales", "en"),
        browser_extension!("ublock-origin",       "uBlock Origin",       github => "gorhill/uBlock",                     "src/_locales",                    "en"),
        browser_extension!("i-still-dont-care-about-cookies", "I still don't care about cookies", github => "OhMyGuus/I-Still-Dont-Care-About-Cookies", "src/_locales", "en"),

        // GeoGebra

        duo!("geogebra-web", "GeoGebra web", Some("GeoGebra"), parse_geogebra_js_json, github => {
            default_url: "geogebra/geogebra/master/web/src/nonfree/resources/org/geogebra/web/pub/js/properties_keys_en.js",
                    url: "geogebra/geogebra/master/web/src/nonfree/resources/org/geogebra/web/pub/js/properties_keys_{}.js",
            lang_id: "-", true, "-", false,
        }),

        geogebra!("geogebra-colors",  "GeoGebra colors",  properties => "colors"),
        geogebra!("geogebra-command", "GeoGebra command", properties => "command"),
        geogebra!("geogebra-error",   "GeoGebra error",   properties => "error"),
        geogebra!("geogebra-javaui",  "GeoGebra javaui",  properties => "javaui"),
        geogebra!("geogebra-menu",    "GeoGebra menu",    properties => "menu"),
        geogebra!("geogebra-symbols", "GeoGebra symbols", properties => "symbols"),

        // Mastodon

        duo!("mastodon-javascript", "Mastodon JavaScript", Some("Mastodon"), parse_json, github => {
            default_url: "mastodon/mastodon/main/app/javascript/mastodon/locales/en.json",
                    url: "mastodon/mastodon/main/app/javascript/mastodon/locales/{}.json",
            lang_id: "-", true, "-", false,
        }),

        duo!("mastodon-android", "Mastodon for Android", Some("Mastodon"), parse_android, github => {
            default_url: "mastodon/mastodon-android/master/mastodon/src/main/res/values/strings.xml",
                    url: "mastodon/mastodon-android/master/mastodon/src/main/res/values-{}/strings.xml",
            lang_id: "-r", true, "-", false,
        }),

        mastodon!("mastodon",              "Mastodon",               yaml => "en.yml",              "{}.yml"),
        mastodon!("mastodon-activerecord", "Mastodon active record", yaml => "activerecord.en.yml", "activerecord.{}.yml"),
        mastodon!("mastodon-devise",       "Mastodon devise",        yaml => "devise.en.yml",       "devise.{}.yml"),
        mastodon!("mastodon-doorkeeper",   "Mastodon doorkeeper",    yaml => "doorkeeper.en.yml",   "doorkeeper.{}.yml"),
        mastodon!("mastodon-simple-form",  "Mastodon simple form",   yaml => "simple_form.en.yml",  "simple_form.{}.yml"),

        // Tor project

        tor!("onion-launchpad",     "Onion Launchpad",     po => "onion-launchpad",                "contents+{}.po"),
        tor!("support-portal",      "Support Portal",      po => "support-portal",                 "contents+{}.po"),
        tor!("torbrowser-manual",   "Tor Browser manual",  po => "tbmanual-contentspot",           "contents+{}.po"),
        tor!("community",           "Tor Community",       po => "communitytpo-contentspot",       "contents+{}.po"),
        tor!("about",               "About Tor Project",   po => "tpo-web",                        "contents+{}.po"),
        tor!("tails-misc",          "Tails miscellaneous", po => "tails-misc",                     "{}.po"),
        tor!("code-of-conduct",     "Code of Conduct",     po => "policies-code_of_conducttxtpot", "code_of_conduct+{}.po"),
        tor!("onion-sprouts-bot",   "Onion Sprouts Bot",   po => "onionsproutsbot",                "onionsproutsbot+{}.po"),
        tor!("tor-animation-title", "Tor Animation title", po => "tor_animation",                  "title-{}.po"),
        tor!("tor-check",           "Tor check",           po => "torcheck",                       "{}/torcheck.po"),

        tor!("torbrowser-about-dialog", "Tor Browser about dialog", dtd => "tor-browser", "aboutDialog.dtd"),
        tor!("torbrowser-about-update", "Tor Browser about update", dtd => "tor-browser", "aboutTBUpdate.dtd"),
        tor!("torbrowser-branding",     "Tor Browser branding",     dtd => "tor-browser", "brand.dtd"),
        tor!("torbrowser-tor-buttons",  "Tor Browser tor buttons",  dtd => "tor-browser", "torbutton.dtd"),

        tor!("onionshare-subtitles",    "OnionShare introduction video",  srt => "onionshare-introduction-video-subtitles", "src/onionshare-introduction.srt",  "onionshare-introduction-subs-{}.srt"),
        tor!("bridges-subtitles",       "Bridges introduction video",     srt => "bridges-introduction-video-subtitles",    "src/bridges-introduction.srt",     "bridges-introduction-subtitles-{}.srt"),
        tor!("torbrowser-subtitles",    "Tor Browser introduction video", srt => "tb-introduction-video-subtitles",         "src/tor-browser-introduction.srt", "tor-browser-sub-{}.srt"),
        tor!("tor-animation-subtitles", "Tor animation",                  srt => "tor_animation",                           "Tor_animation.srt",                "subtitles-{}.srt"),

        tor!("tor-vpn",        "Tor VPN",         android => "tor-vpn",                    "res/values/strings.xml",       "res/values-{}/strings.xml"),
        tor!("torbrowser-app", "Tor Browser App", android => "fenix-torbrowserstringsxml", "en-US/torbrowser_strings.xml", "{}/torbrowser_strings.xml"),

        tor!("torbrowser-brand",                "Tor Browser brand",                properties => "tor-browser",  "brand.properties"),
        tor!("torbrowser-browser-onboarding",   "Tor Browser browser onboarding",   properties => "tor-browser",  "browserOnboarding.properties"),
        tor!("torbrowser-crypto-safety-prompt", "Tor Browser crypto safety prompt", properties => "tor-browser",  "cryptoSafetyPrompt.properties"),
        tor!("torbrowser-onboarding",           "Tor Browser onboarding",           properties => "tor-browser",  "onboarding.properties"),
        tor!("torbrowser-onion-location",       "Tor Browser onion location",       properties => "tor-browser",  "onionLocation.properties"),
        tor!("torbrowser-rulesets",             "Tor Browser rulesets",             properties => "tor-browser",  "rulesets.properties"),
        tor!("torbrowser-settings",             "Tor Browser settings",             properties => "tor-browser",  "settings.properties"),
        tor!("torbrowser-tor-connect",          "Tor Browser tor connect",          properties => "tor-browser",  "torConnect.properties"),
        tor!("torbrowser-torbutton",            "Tor Browser torbutton",            properties => "tor-browser",  "torbutton.properties"),
        tor!("torbrowser-torlauncher",          "Tor Browser torlauncher",          properties => "tor-browser",  "torlauncher.properties"),
        tor!("basebrowser-new-identity",        "Base Browser new identity",        properties => "base-browser", "newIdentity.properties"),
        tor!("basebrowser-security-level",      "Base Browser security level",      properties => "base-browser", "securityLevel.properties"),

        tor!("snowflake",         "Snowflake",         browser_extension => "snowflake", "messages.json"),
        tor!("snowflake-website", "Snowflake website", browser_extension => "snowflake", "website.json"),

        // Elementary

        elementary!("appcenter",         "AppCenter",         po => "appcenter",   "master/po"),
        elementary!("appcenter-extra",   "AppCenter Extra",   po => "appcenter",   "master/po/extra"),
        elementary!("calculator",        "Calculator",        po => "calculator",  "master/po"),
        elementary!("calculator-extra",  "Calculator Extra",  po => "calculator",  "master/po/extra"),
        elementary!("calendar",          "Calendar",          po => "calendar",    "master/po"),
        elementary!("calendar-extra",    "Calendar Extra",    po => "calendar",    "master/po/extra"),
        elementary!("camera",            "Camera",            po => "camera",      "master/po"),
        elementary!("camera-extra",      "Camera Extra",      po => "camera",      "master/po/extra"),
        elementary!("code",              "Code",              po => "code",        "master/po"),
        elementary!("code-extra",        "Code Extra",        po => "code",        "master/po/extra"),
        elementary!("code-plugins",      "Code Plugins",      po => "code",        "master/po/plugins"),
        elementary!("files",             "Files",             po => "files",       "main/po"),
        elementary!("files-extra",       "Files Extra",       po => "files",       "main/po/extra"),
        elementary!("friends",           "Friends",           po => "friends",     "master/po"),
        elementary!("friends-extra",     "Friends Extra",     po => "friends",     "master/po/extra"),
        elementary!("installer",         "Installer",         po => "installer",   "master/po"),
        elementary!("installer-extra",   "Installer Extra",   po => "installer",   "master/po/extra"),
        elementary!("mail",              "Mail",              po => "mail",        "master/po"),
        elementary!("mail-extra",        "Mail Extra",        po => "mail",        "master/po/extra"),
        elementary!("music",             "Music",             po => "music",       "main/po"),
        elementary!("music-extra",       "Music Extra",       po => "music",       "main/po/extra"),
        elementary!("photos",            "Photos",            po => "photos",      "master/po"),
        elementary!("photos-extra",      "Photos Extra",      po => "photos",      "master/po/extra"),
        elementary!("screenshot",        "Screenshot",        po => "screenshot",  "master/po"),
        elementary!("screenshot-extra",  "Screenshot Extra",  po => "screenshot",  "master/po/extra"),
        elementary!("switchboard",       "Switchboard",       po => "switchboard", "main/po"),
        elementary!("switchboard-extra", "Switchboard Extra", po => "switchboard", "main/po/extra"),
        elementary!("tasks",             "Tasks",             po => "tasks",       "master/po"),
        elementary!("tasks-extra",       "Tasks Extra",       po => "tasks",       "master/po/extra"),
        elementary!("terminal",          "Terminal",          po => "terminal",    "master/po"),
        elementary!("terminal-extra",    "Terminal Extra",    po => "terminal",    "master/po/extra"),
        elementary!("videos",            "Videos",            po => "videos",      "main/po"),
        elementary!("videos-extra",      "Videos Extra",      po => "videos",      "main/po/extra"),
        elementary!("wingpanel",         "Wingpanel",         po => "wingpanel",   "master/po"),
        elementary!("wingpanel-extra",   "Wingpanel Extra",   po => "wingpanel",   "master/po/extra"),

        elementary!("capnet-assist",               "Captive Network Assistant",          po => "capnet-assist",         "master/po"),
        elementary!("capnet-assist-extra",         "Captive Network Assistant Extra",    po => "capnet-assist",         "master/po/extra"),
        elementary!("feedback",                    "Feedback",                           po => "feedback",              "master/po"),
        elementary!("feedback-extra",              "Feedback Extra",                     po => "feedback",              "master/po/extra"),
        elementary!("flatpak-platform",            "Flatpak Platform",                   po => "flatpak-platform",      "main/platform-data/po"),
        elementary!("gala",                        "Gala",                               po => "gala",                  "master/po"),
        elementary!("granite",                     "Granite",                            po => "granite",               "main/po"),
        elementary!("granite-extra",               "Granite Extra",                      po => "granite",               "main/po/extra"),
        elementary!("greeter",                     "Greeter",                            po => "greeter",               "master/po"),
        elementary!("greeter-extra",               "Greeter Extra",                      po => "greeter",               "master/po/extra"),
        elementary!("icons",                       "Icons",                              po => "icons",                 "main/po"),
        elementary!("notifications",               "Notifications",                      po => "notifications",         "master/po/extra"),
        elementary!("pantheon-agent-polkit",       "Pantheon Polkit Agent",              po => "pantheon-agent-polkit", "main/po"),
        elementary!("pantheon-agent-polkit-extra", "Pantheon Polkit Agent Extra",        po => "pantheon-agent-polkit", "main/po/extra"),
        elementary!("portals",                     "Pantheon XDG Desktop Portals",       po => "portals",               "main/po"),
        elementary!("portals-extra",               "Pantheon XDG Desktop Portals Extra", po => "portals",               "main/po/extra"),
        elementary!("settings-daemon",             "Settings Daemon",                    po => "settings-daemon",       "master/po"),
        elementary!("shortcut-overlay",            "Shortcut Overlay",                   po => "shortcut-overlay",      "master/po"),
        elementary!("shortcut-overlay-extra",      "Shortcut Overlay Extra",             po => "shortcut-overlay",      "master/po/extra"),
        elementary!("sideload",                    "Sideload",                           po => "sideload",              "master/po"),
        elementary!("sideload-extra",              "Sideload Extra",                     po => "sideload",              "master/po/extra"),
        elementary!("stylesheet",                  "Stylesheet",                         po => "stylesheet",            "master/po"),
        elementary!("wallpapers",                  "Wallpapers",                         po => "wallpapers",            "master/po"),

        elementary!("website-docs-installation",        json => "docs/installation"),
        elementary!("website-docs-learning-the-basics", json => "docs/learning-the-basics"),
        elementary!("website-docs-translation-guide",   json => "docs/translation-guide"),
        elementary!("website-store-index",              json => "store/index"),
        elementary!("website-store-cart",               json => "store/cart"),
        elementary!("website-403",                      json => "403"),
        elementary!("website-404",                      json => "404"),
        elementary!("website-410",                      json => "410"),
        elementary!("website-security",                 json => "SECURITY"),
        elementary!("website-brand",                    json => "brand"),
        elementary!("website-capnet-assist",            json => "capnet-assist"),
        elementary!("website-code-of-conduct",          json => "code-of-conduct"),
        elementary!("website-get-involved",             json => "get-involved"),
        elementary!("website-index",                    json => "index"),
        elementary!("website-layout",                   json => "layout"),
        elementary!("website-oem",                      json => "oem"),
        elementary!("website-open-source",              json => "open-source"),
        elementary!("website-press",                    json => "press"),
        elementary!("website-privacy",                  json => "privacy"),
        elementary!("website-support",                  json => "support"),
        elementary!("website-thank-you",                json => "thank-you"),
    ];

    #[cfg(debug_assertions)]
    {
        use std::collections::HashSet;

        let mut set = HashSet::with_capacity(providers.len());
        for (i, provider) in providers.iter().enumerate() {
            if !provider.code().chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
                panic!("Invalid code: '{}', at index {i}", provider.code());
            }
            if provider.name() == "" {
                panic!("Provider has empty name: '{}', at index {i}", provider.code());
            }

            if set.contains(provider.code()) {
                panic!("Duplicate code: '{}', second at index {i}", provider.code());
            }
            set.insert(provider.code());

            if !provider.name().is_empty() && set.contains(provider.name()) {
                panic!("Duplicate name: '{}', second at index {i}", provider.name());
            }
            set.insert(provider.name());
        }
    }

    providers
}
