//! Compile-time contracts for the temporary legacy public paths.

#[test]
fn legacy_paths_are_aliases_for_shared_core_types() {
    let legacy_format = officecli_hwpx::format::SourceFormat::Hwpx;
    let _: officecli_hancom_core::container::SourceFormat = legacy_format;

    let legacy_document = officecli_hwpx::owpml::model::Document::default();
    let _: officecli_hancom_core::model::Document = legacy_document;

    let legacy_item = officecli_hwpx::emit::batch::BatchItem::add("/body", "paragraph");
    let _: officecli_hancom_core::emit::batch::BatchItem = legacy_item;

    let legacy_error = officecli_hwpx::error::PluginError::corrupt("fixture");
    let _: officecli_hancom_core::error::PluginError = legacy_error;
}
