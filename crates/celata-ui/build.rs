// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

fn main() {
    // Style "fluent": visually close to the Windows client, independent of the system theme.
    // Translations: the texts in `@tr("…")` are English, the other languages are `.po` files below
    // `ui/translations/<code>/LC_MESSAGES/` and are bundled into the program. Without a default context
    // there is exactly one entry per English text.
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-light".into())
        .with_bundled_translations("../../ui/translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("../../ui/main.slint", config).expect("the user interface could not be compiled");
}
