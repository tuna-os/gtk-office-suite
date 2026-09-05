# Translations

The suite uses gettext with the single domain `gtk-office-suite`.
User-facing strings are wrapped in `suite_common::i18n()` at the call
site. `po/POTFILES` tracks the authoritative list of source files containing
`i18n()` calls across `suite-common`, `letters`, `tables`, and `decks`.

Regenerate the template with:

    scripts/update-pot.sh

which reads `po/POTFILES` and writes `po/gtk-office-suite.pot`. Add a language
by copying the pot to `po/<lang>.po`, translating, and listing `<lang>` in
`po/LINGUAS`. `.mo` files install to `<prefix>/share/locale/<lang>/
LC_MESSAGES/gtk-office-suite.mo` (the Flatpak manifests handle this
once .po files exist).
