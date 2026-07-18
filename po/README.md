# Translations

The suite uses gettext with the single domain `gtk-office-suite`.
User-facing strings are wrapped in `suite_common::i18n()` at the call
site. Regenerate the template with:

    scripts/update-pot.sh

which writes `po/gtk-office-suite.pot`. Add a language by copying the
pot to `po/<lang>.po`, translating, and listing `<lang>` in
`po/LINGUAS`. `.mo` files install to `<prefix>/share/locale/<lang>/
LC_MESSAGES/gtk-office-suite.mo` (the Flatpak manifests handle this
once .po files exist).
