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
    scopeList.childNodes.forEach(e => {
        if (!e.firstChild.firstChild.disabled) {
            e.firstChild.firstChild.checked = allScopesCheckbox.checked;
        }
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
                if (scope.id) {
                    scopeNames[scope.id] = { "name": scope.name, "downloaded": scope.downloaded };
                    newScopeFilters[scope.id] = scopeFilters[scope.id] ?? true;
                } else {
                    for (let s of scope.scopes) {
                        scopeNames[s.id] = { "name": s.name, "downloaded": s.downloaded, "groupName": scope.name };
                        newScopeFilters[s.id] = scopeFilters[s.id] ?? true;
                    }
                }
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
 */
function populateScopes(scopes) {
    scopeList.replaceChildren();

    updateCheckbox(allScopesCheckbox, scopeFilters);

    for (let scope of scopes.toSorted((a, b) => a.name > b.name)) {
        let li = document.createElement("li");
        let label = document.createElement("label");

        let input = document.createElement("input");
        input.type = "checkbox";
        if (scope.downloaded === false ||
            (scope.scopes && !scope.scopes.reduce((acc, scope) => acc || scope.downloaded, false)))
        {
            input.disabled = true;
            li.title = `«${scope.name}» er ikkje lasta ned`;
        } else {
            input.checked = scope.id ?
                scopeFilters[scope.id] :
                scope.scopes.reduce((acc, scope) => acc || scopeFilters[scope.id], false);
        }
        input.oninput = e => {
            if (scope.id) {
                scopeFilters[scope.id] = input.checked;
            } else {
                for (let s of scope.scopes) {
                    scopeFilters[s.id] = input.checked;
                }
            }
            updateCheckbox(allScopesCheckbox, scopeFilters);
            refreshTranslations();
        };
        label.appendChild(input);

        let span = document.createElement("span");
        span.appendChild(document.createTextNode(scope.name));
        label.appendChild(span);

        li.appendChild(label)
        scopeList.appendChild(li);
    }
}

function updateCheckbox(checkbox, filters) {
    if (Object.getOwnPropertyNames(filters).length) {
        let checked = Object.values(filters).reduce((acc, checked) => acc === checked ? acc : null);
        if (checked === null) {
            checkbox.checked = true;
            checkbox.indeterminate = true;
        } else {
            checkbox.checked = checked;
            checkbox.indeterminate = false;
        }
    } else {
        checkbox.checked = true;
    }
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
 * Replace the translations in `translationList` with new ones.
 */
function refreshTranslations() {
    const languages = Object.entries(languageFilters).filter(([k, v]) => v).map(([k, v]) => k);
    const scopes = Object.entries(scopeFilters).filter(([k, v]) => v).map(([k, v]) => k);

    if (!languages.length || !scopes.length) {
        populateTranslations([], false);
        setTranslationCountLabel(0, 0);
        return;
    }

    if (translationController) { translationController.abort(); }
    translationController = new AbortController();

    fetch(
        `http://127.0.0.1:2013/query?search=${encodeURIComponent(searchField.value)}&scopes=${scopes.join()}&languages=${languages.join()}&limit=${downloadAtOnce}`,
        { signal: translationController.signal },
    ).then(
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

    titles.classList.add("loading");
    fetch(
        `http://127.0.0.1:2013/query?search=${encodeURIComponent(searchField.value)}&scopes=${scopes.join()}&languages=${languages.join()}&count=true`,
        { signal: translationController.signal },
    ).then(
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

    const languages = Object.entries(languageFilters).filter(([k, v]) => v).map(([k, v]) => k);
    const scopes = Object.entries(scopeFilters).filter(([k, v]) => v).map(([k, v]) => k);

    if (!languages.length || !scopes.length) {
        return;
    }

    fetch(
        `http://127.0.0.1:2013/query?search=${encodeURIComponent(searchField.value)}&scopes=${scopes.join()}&languages=${languages.join()}&skip=${translationCount}&limit=${downloadAtOnce}`,
        { signal: translationController.signal },
    ).then(
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

        let translationText = document.createElement("p");
        translationText.classList.add("translationText");

        embolden(originalText, translation.original);
        embolden(translationText, translation.translation);

        let scopeText = document.createElement("p");
        scopeText.classList.add("scopeText");
        if (scope.groupName) {
            scopeText.innerText = `${scope.groupName} • ${translation.language}`;
        } else if (scope?.name) {
            scopeText.innerText = `${scope.name} • ${translation.language}`;
        } else {
            scopeText.innerText = `${translation.language}`;
        }

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

    let img = document.createElement("img");
    img.src = "/icon/remove.svg";
    button.appendChild(img);

    li.appendChild(button);
    errors.insertBefore(li, errors.firstChild);
}
