// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::TranslationMessages;

pub fn parse_srt(text: String) -> anyhow::Result<TranslationMessages> {
    let mut subtitles = Vec::new();

    let mut current_subtitle = String::new();
    let mut skip = 2;
    for line in text.lines() {
        if skip > 0 {
            if !line.is_empty() {
                skip -= 1;
            }
            continue;
        }

        if line.is_empty() {
            subtitles.push(current_subtitle);
            current_subtitle = String::new();
            skip = 2;
            continue;
        }

        if !current_subtitle.is_empty() {
            current_subtitle.push('\n');
        }
        current_subtitle.push_str(line);
    }
    if !current_subtitle.is_empty() {
        subtitles.push(current_subtitle);
    }

    let messages = subtitles
        .into_iter()
        .enumerate()
        .map(|(i, subtitle)| (i.to_string(), (subtitle, None)))
        .collect();

    Ok(messages)
}
