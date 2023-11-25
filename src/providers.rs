mod android;
mod browser_extension;
mod minecraft;
mod po;

use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Debug, Formatter},
    sync::Arc,
    vec,
};

use async_trait::async_trait;
use reqwest::Client;
use unic_langid::LanguageIdentifier;

pub use self::{
    android::AndroidHttpProvider,
    browser_extension::BrowserExtensionProvider,
    minecraft::MinecraftProvider,
    po::gnome::graphql_gnome,
    po::kde::graphql_kde,
    po::PoProvider,
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
    ) -> Result<HashMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error>;
}

impl Debug for dyn TranslationProvider + Send + Sync {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "provider({})", self.id())
    }
}

macro_rules! android {
    ($name:literal, $repo:literal, $folder:literal) => {
        Arc::new(AndroidHttpProvider {
            id: concat!($repo, "/", $folder),
            name: $name,
            group_name: Some("Android"),
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
                    lang_id.language.as_str()
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
            url: |lang_id| {
                format!(
                    concat!(
                        "https://github.com/",
                        $repo,
                        "/raw/",
                        $folder,
                        "/{}/messages.json",
                    ),
                    lang_id.language.as_str()
                )
            },
        })
    };
}

#[rustfmt::skip]
pub fn default_providers() -> Vec<Arc<dyn TranslationProvider + Send + Sync>> {
    let providers: Vec<Arc<dyn TranslationProvider + Send + Sync>> = vec![
        Arc::new(MinecraftProvider),
        Arc::new(PoProvider {
            id: "gnome",
            name: "GNOME",
            urls: graphql_gnome,
            remove_char: Some('_'),
        }),
        Arc::new(PoProvider {
            id: "kde",
            name: "KDE",
            urls: graphql_kde,
            remove_char: Some('&'),
        }),

        browser_extension!("Tampermonkey",        github => "Tampermonkey/tampermonkey",   "master/i18n"),
        browser_extension!("Tree Style Tab",      github => "piroor/treestyletab",         "trunk/webextensions/_locales"),
        browser_extension!("Tab Session Manager", github => "sienori/Tab-Session-Manager", "master/src/_locales"),

        android!("", "bootable/recovery",                         "tools/recovery_l10n/res"),
        android!("", "development",                               "apps/Fallback/res"),
        android!("", "device/generic/vulkan-cereal",              "third-party/angle/src/android_system_settings/res"),
        android!("", "device/google/atv",                         "FrameworkPackageStubs/res"),
        android!("", "device/google/atv",                         "libraries/BluetoothServices/res"),
        android!("", "device/google/atv",                         "overlay/TvFrameworkOverlay/res"),
        android!("", "device/google/gs101",                       "overlay-vendor/vendor/google/apps/SetupWizard/res"),
        android!("", "frameworks/base",                           "core/res/res"),
        android!("", "frameworks/base",                           "libs/WindowManager/Shell/res"),
        android!("", "frameworks/base",                           "packages/BackupRestoreConfirmation/res"),
        android!("", "frameworks/base",                           "packages/CarrierDefaultApp/res"),
        android!("", "frameworks/base",                           "packages/CompanionDeviceManager/res"),
        android!("", "frameworks/base",                           "packages/DynamicSystemInstallationService/res"),
        android!("", "frameworks/base",                           "packages/ExternalStorageProvider/res"),
        android!("", "frameworks/base",                           "packages/FusedLocation/res"),
        android!("", "frameworks/base",                           "packages/InputDevices/res"),
        android!("", "frameworks/base",                           "packages/PackageInstaller/res"),
        android!("", "frameworks/base",                           "packages/PrintSpooler/res"),
        android!("", "frameworks/base",                           "packages/SettingsLib/BannerMessagePreference/res"),
        android!("", "frameworks/base",                           "packages/SettingsLib/FooterPreference/res"),
        android!("", "frameworks/base",                           "packages/SettingsLib/HelpUtils/res"),
        android!("", "frameworks/base",                           "packages/SettingsLib/RestrictedLockUtils/res"),
        android!("", "frameworks/base",                           "packages/SettingsLib/SearchWidget/res"),
        android!("", "frameworks/base",                           "packages/SettingsLib/SelectorWithWidgetPreference/res"),
        android!("", "frameworks/base",                           "packages/SettingsLib/res"),
        android!("", "frameworks/base",                           "packages/SettingsProvider/res"),
        android!("", "frameworks/base",                           "packages/Shell/res"),
        android!("", "frameworks/base",                           "packages/SimAppDialog/res"),
        android!("", "frameworks/base",                           "packages/SoundPicker/res"),
        android!("", "frameworks/base",                           "packages/SystemUI/res-keyguard"),
        android!("", "frameworks/base",                           "packages/SystemUI/res-product"),
        android!("", "frameworks/base",                           "packages/SystemUI/res"),
        android!("", "frameworks/base",                           "packages/VpnDialogs/res"),
        android!("", "frameworks/base",                           "packages/WallpaperCropper/res"),
        android!("", "frameworks/base",                           "packages/overlays/AvoidAppsInCutoutOverlay/res"),
        android!("", "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationCornerOverlay/res"),
        android!("", "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationDoubleOverlay/res"),
        android!("", "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationHoleOverlay/res"),
        android!("", "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationNarrowOverlay/res"),
        android!("", "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationTallOverlay/res"),
        android!("", "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationWaterfallOverlay/res"),
        android!("", "frameworks/base",                           "packages/overlays/DisplayCutoutEmulationWideOverlay/res"),
        android!("", "frameworks/base",                           "packages/overlays/NoCutoutOverlay/res"),
        android!("", "frameworks/opt/chips",                      "res"),
        android!("", "frameworks/opt/chips",                      "sample/res"),
        android!("", "frameworks/opt/colorpicker",                "res"),
        android!("", "frameworks/opt/net/wifi",                   "libs/WifiTrackerLib/res"),
        android!("", "frameworks/opt/photoviewer",                "res"),
        android!("", "frameworks/opt/photoviewer",                "sample/res"),
        android!("", "frameworks/opt/setupwizard",                "library/main/res"),
        android!("", "frameworks/opt/timezonepicker",             "res"),
        android!("", "packages/apps/BasicSmsReceiver",            "res"),
        android!("", "packages/apps/Calendar",                    "res"),
        android!("", "packages/apps/Camera2",                     "res"),
        android!("", "packages/apps/Car/Calendar",                "res"),
        android!("", "packages/apps/Car/Launcher",                "res"),
        android!("", "packages/apps/Car/LinkViewer",              "res"),
        android!("", "packages/apps/Car/Notification",            "res"),
        android!("", "packages/apps/Car/Settings",                "res"),
        android!("", "packages/apps/Car/SystemUI",                "res"),
        android!("", "packages/apps/Car/SystemUpdater",           "res"),
        android!("", "packages/apps/Car/systemlibs",              "car-assist-client-lib/res"),
        android!("", "packages/apps/Car/systemlibs",              "car-broadcastradio-support/res"),
        android!("", "packages/apps/CellBroadcastReceiver",       "res"),
        android!("", "packages/apps/CertInstaller",               "res"),
        android!("", "packages/apps/Contacts",                    "res"),
        android!("", "packages/apps/DeskClock",                   "res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/about/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/app/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/assisteddialing/ui/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/blocking/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/blockreportspam/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/callcomposer/cameraui/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/callcomposer/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/calldetails/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/calllog/ui/menu/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/calllog/ui/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/calllogutils/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/clipboard/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/common/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/contactphoto/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/dialpadview/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/glidephotomanager/impl/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/historyitemactions/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/interactions/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/main/impl/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/notification/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/phonenumberutil/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/postcall/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/precall/impl/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/preferredsim/impl/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/preferredsim/suggestion/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/promotion/impl/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/cp2/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/directories/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/list/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/searchfragment/nearbyplaces/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/shortcuts/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/spam/promo/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/spannable/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/speeddial/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/theme/common/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/util/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/voicemail/listui/error/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/voicemail/settings/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/dialer/widget/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/answer/impl/answermethod/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/answer/impl/hint/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/answer/impl/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/audioroute/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/commontheme/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/contactgrid/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/disconnectdialog/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/hold/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/incall/impl/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/rtt/impl/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/sessiondata/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/spam/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/telecomeventui/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/theme/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/incallui/video/impl/res"),
        android!("", "packages/apps/Dialer",                      "java/com/android/voicemail/impl/res"),
        android!("", "packages/apps/DocumentsUI",                 "res"),
        android!("", "packages/apps/EmergencyInfo",               "EmergencyGestureAction/res"),
        android!("", "packages/apps/EmergencyInfo",               "res"),
        android!("", "packages/apps/Gallery",                     "res"),
        android!("", "packages/apps/Gallery2",                    "res"),
        android!("", "packages/apps/HTMLViewer",                  "res"),
        android!("", "packages/apps/KeyChain",                    "res"),
        android!("", "packages/apps/Launcher3",                   "go/quickstep/res"),
        android!("", "packages/apps/Launcher3",                   "quickstep/res"),
        android!("", "packages/apps/Launcher3",                   "res"),
        android!("", "packages/apps/LegacyCamera",                "res"),
        android!("", "packages/apps/ManagedProvisioning",         "res"),
        android!("", "packages/apps/Messaging",                   "res"),
        android!("", "packages/apps/Music",                       "kotlin/res"),
        android!("", "packages/apps/MusicFX",                     "res"),
        android!("", "packages/apps/Nfc",                         "res"),
        android!("", "packages/apps/PhoneCommon",                 "res"),
        android!("", "packages/apps/Protips",                     "res"),
        android!("", "packages/apps/QuickAccessWallet",           "res"),
        android!("", "packages/apps/SafetyRegulatoryInfo",        "res"),
        android!("", "packages/apps/Settings",                    "res"),
        android!("", "packages/apps/SettingsIntelligence",        "res"),
        android!("", "packages/apps/Stk",                         "res"),
        android!("", "packages/apps/StorageManager",              "res"),
        android!("", "packages/apps/TV",                          "common/res"),
        android!("", "packages/apps/TV",                          "res"),
        android!("", "packages/apps/Tag",                         "res"),
        android!("", "packages/apps/ThemePicker",                 "res"),
        android!("", "packages/apps/Traceur",                     "res"),
        android!("", "packages/apps/TvSettings",                  "Settings/res-twopanel"),
        android!("", "packages/apps/TvSettings",                  "Settings/res"),
        android!("", "packages/apps/TvSettings",                  "TwoPanelSettingsLib/res"),
        android!("", "packages/apps/WallpaperPicker",             "res"),
        android!("", "packages/apps/WallpaperPicker2",            "res"),
        android!("", "packages/inputmethods/LatinIME",            "java/res"),
        android!("", "packages/inputmethods/LeanbackIME",         "res"),
        android!("", "packages/modules/Bluetooth",                "android/app/res"),
        android!("", "packages/modules/CaptivePortalLogin",       "res"),
        android!("", "packages/modules/CellBroadcastService",     "res"),
        android!("", "packages/modules/Connectivity",             "Tethering/res"),
        android!("", "packages/modules/Connectivity",             "nearby/halfsheet/res"),
        android!("", "packages/modules/Connectivity",             "service/ServiceConnectivityResources/res"),
        android!("", "packages/modules/ExtServices",              "java/res"),
        android!("", "packages/modules/NetworkStack",             "res"),
        android!("", "packages/modules/Permission",               "PermissionController/res"),
        android!("", "packages/modules/Permission",               "SafetyCenter/Resources/res"),
        android!("", "packages/modules/Wifi",                     "OsuLogin/res"),
        android!("", "packages/modules/Wifi",                     "service/ServiceWifiResources/res"),
        android!("", "packages/providers/BlockedNumberProvider",  "res"),
        android!("", "packages/providers/CalendarProvider",       "res"),
        android!("", "packages/providers/ContactsProvider",       "res"),
        android!("", "packages/providers/DownloadProvider",       "res"),
        android!("", "packages/providers/DownloadProvider",       "ui/res"),
        android!("", "packages/providers/MediaProvider",          "res"),
        android!("", "packages/providers/TelephonyProvider",      "res"),
        android!("", "packages/providers/TvProvider",             "res"),
        android!("", "packages/providers/UserDictionaryProvider", "res"),
        android!("", "packages/screensavers/Basic",               "res"),
        android!("", "packages/screensavers/PhotoTable",          "res"),
        android!("", "packages/services/BuiltInPrintService",     "res"),
        android!("", "packages/services/Car",                     "FrameworkPackageStubs/res"),
        android!("", "packages/services/Car",                     "car-admin-ui-lib/src/main/res"),
        android!("", "packages/services/Car",                     "car-maps-placeholder/res"),
        android!("", "packages/services/Car",                     "car-usb-handler/res"),
        android!("", "packages/services/Car",                     "car_product/car_ui_portrait/apps/CarUiPortraitSystemUI/res"),
        android!("", "packages/services/Car",                     "car_product/car_ui_portrait/rro/CarEvsCameraPreviewAppRRO/res"),
        android!("", "packages/services/Car",                     "car_product/car_ui_portrait/rro/CarUiPortraitDialerRRO/res"),
        android!("", "packages/services/Car",                     "car_product/car_ui_portrait/rro/CarUiPortraitLauncherRRO/res"),
        android!("", "packages/services/Car",                     "car_product/car_ui_portrait/rro/CarUiPortraitNotificationRRO/res"),
        android!("", "packages/services/Car",                     "car_product/overlay/frameworks/base/core/res/res"),
        android!("", "packages/services/Car",                     "experimental/service/res"),
        android!("", "packages/services/Car",                     "packages/CarDeveloperOptions/res"),
        android!("", "packages/services/Car",                     "packages/CarManagedProvisioning/res"),
        android!("", "packages/services/Car",                     "service-builtin/res"),
        android!("", "packages/services/Car",                     "service/res"),
        android!("", "packages/services/Car",                     "tests/BugReportApp/res"),
        android!("", "packages/services/Car",                     "tests/DiagnosticTools/res"),
        android!("", "packages/services/Car",                     "tests/MultiDisplaySecondaryHomeTestLauncher/res"),
        android!("", "packages/services/Car",                     "tests/MultiDisplayTest/res"),
        android!("", "packages/services/Car",                     "tests/MultiDisplayTestHelloActivity/res"),
        android!("", "packages/services/Mtp",                     "res"),
        android!("", "packages/services/Telecomm",                "res"),
        android!("", "packages/services/Telephony",               "res"),
        android!("", "packages/services/Telephony",               "testapps/GbaTestApp/res"),
        android!("", "packages/services/Telephony",               "testapps/TestSliceApp/app/src/main/res"),
        android!("", "packages/wallpapers/LivePicker",            "res"),
    ];

    #[cfg(debug_assertions)]
    {
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
