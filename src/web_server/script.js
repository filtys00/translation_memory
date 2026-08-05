// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const header = document.getElementsByTagName("header")[0];
const search = document.getElementById("search");
const searchField = document.getElementById("searchField");
const noTranslations = document.getElementById("noTranslations");
const errors = document.getElementById("errors");

const languagesAndScopes = document.getElementById("languagesAndScopes");
const languagesAndScopesButton = document.getElementById("languagesAndScopesButton");
const languageList = document.getElementById("languages");
const scopeList = document.getElementById("scopes");

const allScopesCheckbox = document.getElementById("allScopesCheckbox");
const allScopesCheckboxLabel = document.getElementById("allScopesCheckboxLabel");
const allLanguagesCheckbox = document.getElementById("allLanguagesCheckbox");
const allLanguagesCheckboxLabel = document.getElementById("allLanguagesCheckboxLabel");

const titles = document.getElementById("titles");
const originalsTitle = document.getElementById("originalsTitle");
const translationsTitle = document.getElementById("translationsTitle");
const translationCountLabel = document.getElementById("translationCount");

const translationList = document.getElementById("translations");

/**
 * @define { <id>: { name: string, downloaded: bool, groupName?: string } }
 */
let scopeNames = {};

/**
 * @define { <name>: bool }
 */
let languageFilters = {};

/**
 * @define { <name>: bool }
 */
let scopeFilters = {};

/**
 * @define [ string ]
 */
let languages = [];

/**
 * @define [ string ]
 */
let newLanguages = [];

document.title = "Omsetjingsminne";
searchField.placeholder = "Søk";
languagesAndScopesButton.innerText = "Mål og vidd";
originalsTitle.innerText = "Opprinneleg";
translationsTitle.innerText = "Omsetjing";
noTranslations.innerText = "Ingen resultat";

function setTranslationCountLabel(count, totalCount) {
    let num = new Intl.NumberFormat("nn");
    translationCountLabel.innerText = `${num.format(totalCount)} omsetjingar`;
    translationCountLabel.title = totalCount === null
        ? `${num.format(count)} av ukjend`
        : `${num.format(count)} av ${num.format(totalCount)}`;
};

allLanguagesCheckboxLabel.innerText = "Alle mål";
allScopesCheckboxLabel.innerText = "Alle vidder";

refreshAll();

// Add listeners to elements

document.addEventListener("visibilitychange", () => {
    if (document.hidden) { return; }
    searchField.select();
    searchField.focus();
});

translationList.onscroll = e => {
    if (e.target.scrollTop === 0) {
        header.classList.remove("floating");
    } else {
        header.classList.add("floating");
    }
    if (Math.abs(e.target.scrollHeight - e.target.clientHeight - e.target.scrollTop) <= 1) {
        appendTranslations();
    }
};

let lastUpdated = Date.now();
searchField.oninput = e => {
    const now = Date.now();
    lastUpdated = now;
    setTimeout(() => {
        if (lastUpdated === now) {
            refreshTranslations();
        }
    }, 250);
};
searchField.onkeypress = e => {
    if (e.key !== "Enter") { return; }
    refreshTranslations();
}
searchField.onkeydown = e => {
    if (!document.execCommand) { return; }
    if (e.key !== '"' && e.key !== "'") { return; }

    let [start, end] = [searchField.selectionStart, searchField.selectionEnd];
    if (start === end) { return; }

    e.preventDefault();

    document.execCommand(
        "insertText",
        undefined,
        `${e.key}${searchField.value.substring(start, end)}${e.key}`,
    );
    searchField.setSelectionRange(start + 1, end + 1);
}

search.onclick = e => {
    if (e.target == searchField) return;
    searchField.focus();
    searchField.setSelectionRange(searchField.value.length, searchField.value.length + 1);
};

window.onmousedown = e => {
    if (languagesAndScopes.classList.contains("loading")) { return; }

    if (!languagesAndScopes.contains(e.target)) {
        languagesAndScopes.classList.remove("open");
    };
};
languagesAndScopesButton.onclick = e => {
    if (languagesAndScopes.classList.contains("loading")) { return; }

    if (languagesAndScopes.classList.contains("open")) {
        languagesAndScopes.classList.remove("open");
    } else {
        languagesAndScopes.classList.add("open");
    }
};

allLanguagesCheckbox.oninput = e => {
    languageList.childNodes.forEach(e => e.firstChild.firstChild.checked = allLanguagesCheckbox.checked);
    for (language in languageFilters) {
        languageFilters[language] = allLanguagesCheckbox.checked;
    }
    refreshTranslations();
}
allScopesCheckbox.oninput = e => {
    scopeList.querySelectorAll("input[type=\"checkbox\"]").forEach(el => {
        el.checked = allScopesCheckbox.checked;
        el.indeterminate = false;
    });
    for (scope in scopeFilters) {
        scopeFilters[scope] = allScopesCheckbox.checked;
    }
    refreshTranslations();
}

// Functions

/**
 * Replace the languages and scopes in `languageList` and `scopeList` with new ones.
 */
function refreshAll() {
    fetch("http://127.0.0.1:2013/metadata").then(
        async response => {
            const metadata = await response.json();

            languages = metadata.languages;

            languageFilters = Object.fromEntries(
                metadata.languages.map(language => [language, languageFilters[language] ?? true])
            );

            scopeNames = {};
            let newScopeFilters = {};
            for (let scope of metadata.scopes) {
                scopeNames[scope.id] = { "name": scope.name, "groupName": scope.groupName };
                newScopeFilters[scope.id] = scopeFilters[scope.id] ?? true;
            }
            scopeFilters = newScopeFilters;

            populateLanguages(metadata.languages);
            populateScopes(metadata.scopes);

            refreshTranslations();
        },
        e => {
            console.error(e);
            pushError(e, "Klarte ikkje oppdatera måla og viddene");
        },
    )
}

/**
 * Replace the contents of `languageList` with `languages`.
 */
function populateLanguages(languages) {
    languageList.replaceChildren();
    
    updateCheckbox(allLanguagesCheckbox, languageFilters);

    for (let lang of languages.toSorted()) {
        let li = document.createElement("li");

        let label = document.createElement("label");

        let input = document.createElement("input");
        input.type = "checkbox";
        languageFilters[lang] = languageFilters[lang] ?? true;
        input.checked = languageFilters[lang];
        input.oninput = () => {
            languageFilters[lang] = input.checked;
            updateCheckbox(allLanguagesCheckbox, languageFilters);
            refreshTranslations();
        };
        label.appendChild(input);

        let span = document.createElement("span");
        span.appendChild(document.createTextNode(lang));
        label.appendChild(span);

        li.appendChild(label);

        languageList.appendChild(li);
    }
}

/**
 * Replace the contents of `scopeList` with `scopes`.
 * 
 * @param {{ id: string, name: string, groupName: string? }[]} scopes
 */
function populateScopes(scopes) {
    scopeList.replaceChildren();

    const allScopeIds = scopes.map(s => s.id);
    updateCheckbox(allScopesCheckbox, scopeFilters, allScopeIds);

    const groups = Object.entries(Object.groupBy(scopes, s => s.groupName ?? s.name))
        .toSorted(([groupNameA], [groupNameB]) => groupNameA > groupNameB);
    for (const [groupName, groupScopes] of groups) {
        if (groupScopes.every(s => s.name === groupScopes[0].name)) {
            const nameScopeIds = scopes.filter(s => s.name === groupName).map(s => s.id);
            
            // Create single scope entry
            let li = document.createElement("li");
            scopeList.appendChild(li);
            let label = document.createElement("label");
            li.appendChild(label);

            let checkbox = document.createElement("input");
            checkbox.type = "checkbox";
            updateCheckbox(checkbox, scopeFilters, nameScopeIds);
            checkbox.oninput = () => {
                for (let id of nameScopeIds) {
                    scopeFilters[id] = checkbox.checked;
                }
                updateCheckbox(allScopesCheckbox, scopeFilters, allScopeIds);
                refreshTranslations();
            }
            label.appendChild(checkbox);

            let span = document.createElement("span");
            span.appendChild(document.createTextNode(groupName));
            label.appendChild(span);
        } else {
            const groupScopeIds = scopes.filter(s => s.groupName === groupName).map(s => s.id);

            // Create group <li>
            let groupLi = document.createElement("li");
            scopeList.appendChild(groupLi);
            let groupDetails = document.createElement("details");
            groupLi.appendChild(groupDetails);
            groupDetails.name = "scopes";
            let groupSummary = document.createElement("summary");
            groupDetails.appendChild(groupSummary);
            let groupLabel = document.createElement("label");
            groupSummary.appendChild(groupLabel);
            groupLabel.onclick = e => {
                if (e.target instanceof HTMLInputElement) { return; }
                e.preventDefault();
                groupDetails.toggleAttribute("open");
            }

            let groupCheckbox = document.createElement("input");
            groupCheckbox.type = "checkbox";
            updateCheckbox(groupCheckbox, scopeFilters, groupScopeIds);
            groupCheckbox.oninput = () => {
                for (let id of groupScopeIds) {
                    scopeFilters[id] = groupCheckbox.checked;
                }
                groupUl.querySelectorAll("input[type=\"checkbox\"]").forEach(el => {
                    el.checked = groupCheckbox.checked;
                });
                updateCheckbox(allScopesCheckbox, scopeFilters, allScopeIds);
                refreshTranslations();
            }
            groupLabel.appendChild(groupCheckbox);

            let groupSpan = document.createElement("span");
            groupSpan.appendChild(document.createTextNode(groupName));
            groupLabel.appendChild(groupSpan);

            // Create group entries

            groupDetails.appendChild(document.createElement("hr"));
            let groupUl = document.createElement("ul");
            groupDetails.appendChild(groupUl);

            
            const names = Object.entries(Object.groupBy(groupScopes, s => s.name))
                .toSorted(([nameA], [nameB]) => nameA > nameB);
            for (const [name, scopes] of names) {
                const nameScopes = scopes.map(s => s.id);

                let li = document.createElement("li");
                groupUl.appendChild(li);
                let label = document.createElement("label");
                li.appendChild(label);

                let checkbox = document.createElement("input");
                checkbox.type = "checkbox";
                updateCheckbox(checkbox, scopeFilters, nameScopes);
                checkbox.oninput = () => {
                    for (let id of nameScopes) {
                        scopeFilters[id] = checkbox.checked;
                    }
                    updateCheckbox(allScopesCheckbox, scopeFilters);
                    updateCheckbox(groupCheckbox, scopeFilters, groupScopeIds);
                    refreshTranslations();
                }
                label.appendChild(checkbox);

                let span = document.createElement("span");
                span.appendChild(document.createTextNode(name));
                label.appendChild(span);
            }
            groupDetails.appendChild(document.createElement("hr"));
        }
    }
}

/**
 * Update `checkbox` according to weather `ids` in `filters` are true or false.
 * 
 * @param {HTMLInputElement} checkbox
 * @param {{ [id: string]: boolean }} filters
 * @param {string[]} [ids] will be all the ids in `filters` by default
 */
function updateCheckbox(checkbox, filters, ids) {
    if (!ids) { ids = Object.keys(filters); }
    let isChecked = ids.length === 0 || ids.some(id => filters[id] ?? true);
    let isIndeterminate = isChecked && ids.some(id => !(filters[id] ?? true));
    checkbox.checked = isChecked;
    checkbox.indeterminate = isIndeterminate;
}

/**
 * @typedef Translation
 * @type {object}
 * @property original {string} - original string
 * @property translation {string} - translated string
 * @property comment {string} - comment describing the context regarding the translation
 */

/** How many translations should be downloaded per request */
const downloadAtOnce = 100;

/**
 * How many translations the current search has
 * @type number?
 */
let totalTranslationCount = null;

/** How many translations have been downloaded */
let translationCount = 0;

/**
 * An `AbortController` for requesting translations.
 * @type AbortController?
 */
let translationController = null;

/**
 * Returns a `FormData` containing the query paramteres,
 * or `null` if the parameters are guaranteed to return no results.
 * @returns {FormData|null}
 */
function assembleQueryFormData() {
    const data = new FormData();

    if (searchField.value) { data.set('search', searchField.value); }
    data.set('limit', downloadAtOnce);

    let { true: yes_scopes, false: no_scopes } = Object.groupBy(Object.entries(scopeFilters), ([k, v]) => v);
    if (!yes_scopes) { return null; }
    if (!no_scopes) {
    } else if (yes_scopes.length <= no_scopes.length) {
        data.set('require_scopes', yes_scopes.map(([k, v]) => k).join());
    } else {
        data.set('deny_scopes', no_scopes.map(([k, v]) => k).join());
    }

    let { true: yes_langs, false: no_langs } = Object.groupBy(Object.entries(languageFilters), ([k, v]) => v);
    if (!yes_langs) { return null; }
    if (!no_langs) {
    } else if (yes_langs.length <= no_langs.length) {
        data.set('require_languages', yes_langs.map(([k, v]) => k).join());
    } else {
        data.set('deny_languages', no_langs.map(([k, v]) => k).join());
    }

    return data;
}

/**
 * Replace the translations in `translationList` with new ones.
 */
function refreshTranslations() {
    const data = assembleQueryFormData();

    if (data === null) {
        populateTranslations([], false);
        setTranslationCountLabel(0, 0);
        return;
    }

    if (translationController) { translationController.abort(); }
    translationController = new AbortController();

    fetch('http://127.0.0.1:2013/query?' + new URLSearchParams(data), { signal: translationController.signal }).then(
        async response => {
            const translations = await response.json();

            populateTranslations(translations, false);

            window.scrollTo(0, 0);
        },
        e => {
            if (e.name === "AbortError") { return; }
            console.error(e);
            pushError(e, "Klarte ikkje oppdatera omsetjingane");
        },
    );

    data.set('count', true);

    titles.classList.add("loading");
    fetch('http://127.0.0.1:2013/query?' + new URLSearchParams(data), { signal: translationController.signal }).then(
        async response => {
            const count = await response.json();

            totalTranslationCount = count;
            setTranslationCountLabel(translationCount, count);

            titles.classList.remove("loading");
        },
        e => {
            if (e.name === "AbortError") { return; }
            console.error(e);

            totalTranslationCount = null;
            setTranslationCountLabel(translationCount, null);

            titles.classList.remove("loading");
        },
    );
}

function appendTranslations() {
    if (totalTranslationCount !== null && translationCount >= totalTranslationCount) {
        return;
    }

    const form = assembleQueryFormData();
    if (form === null) { return; }
    form.set('skip', translationCount);

    fetch('http://127.0.0.1:2013/query?' + new URLSearchParams(form), { signal: translationController.signal }).then(
        async response => {
            const translations = await response.json();

            populateTranslations(translations, true);
        },
        e => {
            if (e.name === "AbortError") { return; }
            console.error(e);
            pushError(e, "Klarte ikkje leggja til fleire omsetjingar");
        },
    );
}

/**
 * Replace the contents of `translationList` with `translations`.
 * 
 * @param translations {[Translation]} - list of translations to use
 * @param append {boolean?} - weather to append instead of replacing
 */
function populateTranslations(translations, append) {
    if (append) {
        translationCount += translations.length;
    } else {
        translationCount = translations.length;

        translationList.replaceChildren();

        if (translations.length) {
            document.body.classList.remove("noResults");
        } else {
            document.body.classList.add("noResults");
        }
    }
    setTranslationCountLabel(translationCount, totalTranslationCount)

    for (let translation of translations) {
        const scope = scopeNames[translation.scope];

        let li = document.createElement("li");
        li.title = `${translation.language} - ${scope?.name ?? translation.scope}`
                     + `${scope?.groupName ? ` (${scope.groupName})` : ""}`
                 + `\nKjelde: ${translation.source}`
                 + `${translation.key ? `\nNøkkel: ${translation.key}` : ""}`
                 + `${translation.comment ? `\n${translation.comment}` : ""}`;
        li.classList.add("translation");

        let originalText = document.createElement("p");
        originalText.classList.add("originalText");
        embolden(originalText, translation.original);

        let translationText = document.createElement("p");
        translationText.classList.add("translationText");
        embolden(translationText, translation.translation);

        let scopeText = document.createElement("p");
        scopeText.classList.add("scopeText");
        scopeText.innerText = `${scope?.name ?? translation.scope} • ${translation.language}`;

        li.appendChild(originalText);
        li.appendChild(translationText);
        li.appendChild(scopeText);
        translationList.appendChild(li);
    }
}

/**
 * Append `values` to `container`, highlighting any value with `marked` set to `true`.
 * 
 * @param container {HTMLElement} - element to append the formatted `values` into
 * @param values {[string|{marked:bool,text:string}]} - list of values to format
 */
function embolden(container, values) {
    for (const value of values) {
        if (value.marked) {
            let b = document.createElement("b");
            const text = value.text.split("\n");
            b.appendChild(document.createTextNode(text[0]));
            for (let i = 1; i < text.length; i++) {
                b.appendChild(document.createElement("br"));
                b.appendChild(document.createTextNode(text[i]));
            }
            container.appendChild(b);
        } else {
            const text = value.split("\n");
            container.appendChild(document.createTextNode(text[0]));
            for (let i = 1; i < text.length; i++) {
                container.appendChild(document.createElement("br"));
                container.appendChild(document.createTextNode(text[i]));
            }
        }
    }
}

/**
 * Push an error message to the user.
 * 
 * @param msg {string|Error} - an error message
 * @param title {string} - the error title
 */
function pushError(msg, title) {
    if (msg instanceof Error) {
        if (msg.name === "TypeError" && msg.message === "NetworkError when attempting to fetch resource.") {
            msg = "Får ikkje kontakt med bakenden";
        } else {
            msg = msg.name + ": " + msg.message;
        }
    }

    let li = document.createElement("li");
    li.classList.add("created");
    setTimeout(() => li.classList.remove("created"), 100);

    let div = document.createElement("div");

    let b = document.createElement("b");
    b.appendChild(document.createTextNode(title + ":"));
    div.appendChild(b);

    let p = document.createElement("p");
    p.appendChild(document.createTextNode(msg));
    div.appendChild(p);

    li.appendChild(div);

    let button = document.createElement("button");
    button.onclick = e => {
        li.classList.add("removed");
        for (const child of errors.children) {
            if (child === li) { break; }
            child.classList.add("down");
        }
        setTimeout(() => {
            errors.childNodes.forEach(node => node.classList.remove("down"));
            errors.removeChild(li);
        }, 100);
    };

    let span = document.createElement("span");
    span.textContent = "🗙";
    button.appendChild(span);

    li.appendChild(button);
    errors.insertBefore(li, errors.firstChild);
}
