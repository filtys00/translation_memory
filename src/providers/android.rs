// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use anyhow::{anyhow, bail};
use base64::{
    Engine,
    alphabet::Alphabet,
    engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig},
};
use quick_xml::{Reader, events::Event};

use super::{Downloader, LangId, TranslationMessages, SourceUrls, unescape};

const BASE64: GeneralPurpose = GeneralPurpose::new(
    match &Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/") {
        Ok(alphabet) => alphabet,
        Err(_) => unreachable!(),
    },
    GeneralPurposeConfig::new(),
);

pub fn parse_android_base64(base64: String) -> anyhow::Result<TranslationMessages> {
    let bytes = BASE64
        .decode(&base64)
        .map_err(|e| anyhow!("Invalid base64: {e}\n{base64}"))?;
    let text =
        String::from_utf8(bytes).map_err(|e| anyhow!("Invalid text from base64: {e}\n{base64}"))?;
    parse_android(text)
}

pub fn parse_android(text: String) -> anyhow::Result<TranslationMessages> {
    let mut messages = HashMap::new();

    let mut reader = Reader::from_str(&text);
    let mut comment: Option<String> = None;
    let mut key: Option<String> = None;
    let mut message = String::new();
    loop {
        match reader.read_event() {
            Err(e) => bail!("{e}"),
            Ok(Event::Eof) => break,
            Ok(Event::Comment(e)) => {
                comment = Some(String::from_utf8_lossy(&e).trim().to_string());
            }
            Ok(Event::Start(e)) if e.name().as_ref() == b"string" => {
                if e.attributes()
                    .filter_map(|attr| attr.ok())
                    .find(|attr| attr.key.as_ref() == b"translatable")
                    .is_some_and(|attr| attr.value.as_ref() == b"false")
                {
                    comment = None;
                    continue;
                };
                let Some(name_attr) = e
                    .attributes()
                    .filter_map(|attr| attr.ok())
                    .find(|attr| attr.key.as_ref() == b"name")
                    .and_then(|attr| String::from_utf8(attr.value.to_vec()).ok())
                else {
                    comment = None;
                    continue;
                };
                key = Some(name_attr);
            }
            Ok(Event::Text(e)) if key.is_some() => {
                message.push_str(String::from_utf8_lossy(&e).trim());
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"string" => {
                let Some(key_local) = key else {
                    comment = None;
                    message.clear();
                    continue;
                };

                if message.len() > 2
                    && message.starts_with('"')
                    && message.ends_with('"')
                    && !message.ends_with("\\\"")
                    && !message[1..(message.len() - 1)].contains('"')
                {
                    message.remove(message.len() - 1);
                    message.remove(0);
                }
                message = unescape(&message, &['n', 'u', 't', '"', '\'', '‘', '’', '?', '%']);

                messages.insert(key_local, (message, comment));

                comment = None;
                key = None;
                message = String::new();
            }
            _ if key.is_some() => comment = None,
            _ => {}
        }
    }

    Ok(messages)
}

pub fn android_urls<'a>(
    lang_ids: &'a [LangId],
    _: &Downloader,
) -> anyhow::Result<HashMap<&'a LangId, Vec<SourceUrls>>> {
    #[rustfmt::skip]
    let files = [
        ("bootable/recovery", "tools/recovery_l10n/res"),
        ("development",       "apps/Fallback/res"),
        ("frameworks/base", "core/res/res"),
        ("frameworks/base", "libs/WindowManager/Shell/res"),
        ("frameworks/base", "packages/BackupRestoreConfirmation/res"),
        ("frameworks/base", "packages/CarrierDefaultApp/res"),
        ("frameworks/base", "packages/CompanionDeviceManager/res"),
        ("frameworks/base", "packages/DynamicSystemInstallationService/res",),
        ("frameworks/base", "packages/ExternalStorageProvider/res"),
        ("frameworks/base", "packages/FusedLocation/res"),
        ("frameworks/base", "packages/InputDevices/res"),
        ("frameworks/base", "packages/PackageInstaller/res"),
        ("frameworks/base", "packages/PrintSpooler/res"),
        ("frameworks/base", "packages/SettingsLib/BannerMessagePreference/res",),
        ("frameworks/base", "packages/SettingsLib/FooterPreference/res",),
        ("frameworks/base", "packages/SettingsLib/HelpUtils/res"),
        ("frameworks/base", "packages/SettingsLib/RestrictedLockUtils/res",),
        ("frameworks/base", "packages/SettingsLib/SearchWidget/res"),
        ("frameworks/base", "packages/SettingsLib/SelectorWithWidgetPreference/res",),
        ("frameworks/base", "packages/SettingsLib/res"),
        ("frameworks/base", "packages/SettingsProvider/res"),
        ("frameworks/base", "packages/Shell/res"),
        ("frameworks/base", "packages/SimAppDialog/res"),
        ("frameworks/base", "packages/SoundPicker/res"),
        ("frameworks/base", "packages/SystemUI/res-keyguard"),
        ("frameworks/base", "packages/SystemUI/res-product"),
        ("frameworks/base", "packages/SystemUI/res"),
        ("frameworks/base", "packages/VpnDialogs/res"),
        ("frameworks/base", "packages/WallpaperCropper/res"),
        ("frameworks/base", "packages/overlays/AvoidAppsInCutoutOverlay/res"),
        ("frameworks/base", "packages/overlays/DisplayCutoutEmulationCornerOverlay/res"),
        ("frameworks/base", "packages/overlays/DisplayCutoutEmulationDoubleOverlay/res"),
        ("frameworks/base", "packages/overlays/DisplayCutoutEmulationHoleOverlay/res"),
        ("frameworks/base", "packages/overlays/DisplayCutoutEmulationNarrowOverlay/res"),
        ("frameworks/base", "packages/overlays/DisplayCutoutEmulationTallOverlay/res"),
        ("frameworks/base", "packages/overlays/DisplayCutoutEmulationWaterfallOverlay/res"),
        ("frameworks/base", "packages/overlays/DisplayCutoutEmulationWideOverlay/res"),
        ("frameworks/base", "packages/overlays/NoCutoutOverlay/res"),
        ("frameworks/opt/chips",                "res"),
        ("frameworks/opt/chips",                "sample/res"),
        ("frameworks/opt/colorpicker",          "res"),
        ("frameworks/opt/net/wifi",             "libs/WifiTrackerLib/res"),
        ("frameworks/opt/photoviewer",          "res"),
        ("frameworks/opt/photoviewer",          "sample/res"),
        ("frameworks/opt/setupwizard",          "library/main/res"),
        ("frameworks/opt/timezonepicker",       "res"),
        ("packages/apps/BasicSmsReceiver",      "res"),
        ("packages/apps/Calendar",              "res"),
        ("packages/apps/Camera2",               "res"),
        ("packages/apps/Car/Calendar",          "res"),
        ("packages/apps/Car/Launcher",          "app/res"),
        ("packages/apps/Car/LinkViewer",        "res"),
        ("packages/apps/Car/Notification",      "res"),
        ("packages/apps/Car/Settings",          "res"),
        ("packages/apps/Car/SystemUI",          "res"),
        ("packages/apps/Car/SystemUpdater",     "res"),
        ("packages/apps/Car/systemlibs",        "car-assist-client-lib/res"),
        ("packages/apps/Car/systemlibs",        "car-broadcastradio-support/res"),
        ("packages/apps/CellBroadcastReceiver", "res"),
        ("packages/apps/CertInstaller",         "res"),
        ("packages/apps/Contacts",              "res"),
        ("packages/apps/DeskClock",             "res"),
        ("packages/apps/Dialer", "java/com/android/dialer/about/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/app/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/assisteddialing/ui/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/blocking/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/blockreportspam/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/callcomposer/cameraui/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/callcomposer/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/calldetails/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/calllog/ui/menu/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/calllog/ui/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/calllogutils/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/clipboard/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/common/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/contactphoto/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/dialpadview/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/glidephotomanager/impl/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/historyitemactions/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/interactions/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/main/impl/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/notification/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/phonenumberutil/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/postcall/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/precall/impl/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/preferredsim/impl/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/preferredsim/suggestion/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/promotion/impl/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/searchfragment/cp2/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/searchfragment/directories/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/searchfragment/list/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/searchfragment/nearbyplaces/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/shortcuts/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/spam/promo/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/spannable/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/speeddial/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/theme/common/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/util/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/voicemail/listui/error/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/voicemail/settings/res"),
        ("packages/apps/Dialer", "java/com/android/dialer/widget/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/answer/impl/answermethod/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/answer/impl/hint/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/answer/impl/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/audioroute/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/commontheme/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/contactgrid/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/disconnectdialog/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/hold/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/incall/impl/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/rtt/impl/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/sessiondata/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/spam/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/telecomeventui/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/theme/res"),
        ("packages/apps/Dialer", "java/com/android/incallui/video/impl/res"),
        ("packages/apps/Dialer", "java/com/android/voicemail/impl/res"),
        ("packages/apps/DocumentsUI",                 "res"),
        ("packages/apps/EmergencyInfo",               "EmergencyGestureAction/res"),
        ("packages/apps/EmergencyInfo",               "res"),
        ("packages/apps/Gallery",                     "res"),
        ("packages/apps/Gallery2",                    "res"),
        ("packages/apps/HTMLViewer",                  "res"),
        ("packages/apps/KeyChain",                    "res"),
        ("packages/apps/Launcher3",                   "go/quickstep/res"),
        ("packages/apps/Launcher3",                   "quickstep/res"),
        ("packages/apps/Launcher3",                   "res"),
        ("packages/apps/LegacyCamera",                "res"),
        ("packages/apps/ManagedProvisioning",         "res"),
        ("packages/apps/Messaging",                   "res"),
        ("packages/apps/Music",                       "kotlin/res"),
        ("packages/apps/MusicFX",                     "res"),
        ("packages/apps/Nfc",                         "res"),
        ("packages/apps/PhoneCommon",                 "res"),
        ("packages/apps/Protips",                     "res"),
        ("packages/apps/QuickAccessWallet",           "res"),
        ("packages/apps/SafetyRegulatoryInfo",        "res"),
        ("packages/apps/Settings",                    "res"),
        ("packages/apps/SettingsIntelligence",        "res"),
        ("packages/apps/Stk",                         "res"),
        ("packages/apps/StorageManager",              "res"),
        ("packages/apps/TV",                          "common/res"),
        ("packages/apps/TV",                          "res"),
        ("packages/apps/Tag",                         "res"),
        ("packages/apps/ThemePicker",                 "res"),
        ("packages/apps/Traceur",                     "res"),
        ("packages/apps/TvSettings",                  "Settings/res-twopanel"),
        ("packages/apps/TvSettings",                  "Settings/res"),
        ("packages/apps/TvSettings",                  "TwoPanelSettingsLib/res"),
        ("packages/apps/WallpaperPicker",             "res"),
        ("packages/apps/WallpaperPicker2",            "res"),
        ("packages/inputmethods/LatinIME",            "java/res"),
        ("packages/inputmethods/LeanbackIME",         "res"),
        ("packages/modules/Bluetooth",                "android/app/res"),
        ("packages/modules/CaptivePortalLogin",       "res"),
        ("packages/modules/CellBroadcastService",     "res"),
        ("packages/modules/Connectivity",             "Tethering/res"),
        ("packages/modules/Connectivity",             "service/ServiceConnectivityResources/res"),
        ("packages/modules/ExtServices",              "java/res"),
        ("packages/modules/NetworkStack",             "res"),
        ("packages/modules/Permission",               "PermissionController/res"),
        ("packages/modules/Permission",               "SafetyCenter/Resources/res"),
        ("packages/modules/Wifi",                     "OsuLogin/res"),
        ("packages/modules/Wifi",                     "service/ServiceWifiResources/res"),
        ("packages/providers/BlockedNumberProvider",  "res"),
        ("packages/providers/CalendarProvider",       "res"),
        ("packages/providers/ContactsProvider",       "res"),
        ("packages/providers/DownloadProvider",       "res"),
        ("packages/providers/DownloadProvider",       "ui/res"),
        ("packages/providers/MediaProvider",          "res"),
        ("packages/providers/TelephonyProvider",      "res"),
        ("packages/providers/TvProvider",             "res"),
        ("packages/providers/UserDictionaryProvider", "res"),
        ("packages/screensavers/Basic",               "res"),
        ("packages/screensavers/PhotoTable",          "res"),
        ("packages/services/BuiltInPrintService",     "res"),
        ("packages/services/Car", "FrameworkPackageStubs/res"),
        ("packages/services/Car", "car-admin-ui-lib/src/main/res"),
        ("packages/services/Car", "car-maps-placeholder/res"),
        ("packages/services/Car", "car-usb-handler/res"),
        ("packages/services/Car", "car_product/car_ui_portrait/apps/CarUiPortraitSystemUI/res"),
        ("packages/services/Car", "car_product/car_ui_portrait/rro/CarEvsCameraPreviewAppRRO/res"),
        ("packages/services/Car", "car_product/car_ui_portrait/rro/CarUiPortraitDialerRRO/res"),
        ("packages/services/Car", "car_product/car_ui_portrait/rro/CarUiPortraitNotificationRRO/res"),
        ("packages/services/Car", "experimental/service/res"),
        ("packages/services/Car", "packages/CarDeveloperOptions/res"),
        ("packages/services/Car", "packages/CarManagedProvisioning/res"),
        ("packages/services/Car", "service-builtin/res"),
        ("packages/services/Car", "service/res"),
        ("packages/services/Car", "tests/BugReportApp/res"),
        ("packages/services/Car", "tests/DiagnosticTools/res"),
        ("packages/services/Car", "tests/MultiDisplaySecondaryHomeTestLauncher/res"),
        ("packages/services/Car", "tests/MultiDisplayTest/res"),
        ("packages/services/Car", "tests/MultiDisplayTestHelloActivity/res"),
        ("packages/services/Mtp",          "res"),
        ("packages/services/Telecomm",     "res"),
        ("packages/services/Telephony",    "res"),
        ("packages/services/Telephony",    "testapps/GbaTestApp/res"),
        ("packages/services/Telephony",    "testapps/TestSliceApp/app/src/main/res"),
        ("packages/wallpapers/LivePicker", "res"),
    ];

    let mut urls: HashMap<&LangId, Vec<SourceUrls>> = HashMap::with_capacity(files.len());

    for (repository, folder) in files {
        let default_url = format!(
            "https://android.googlesource.com/platform/{repository}/+/master/{folder}/values/strings.xml?format=TEXT",
        );

        for lang_id in lang_ids {
            urls.entry(lang_id).or_default().push(SourceUrls {
                originals: default_url.clone(),
                translations: format!(
                    "https://android.googlesource.com/platform/{repository}/+/master/{folder}/values-{}/strings.xml?format=TEXT",
                    lang_id.format("-r", true, "-", false),
                ),
            });
        }
    }

    Ok(urls)
}
