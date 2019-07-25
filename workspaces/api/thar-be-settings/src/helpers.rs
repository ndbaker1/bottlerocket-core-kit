// This module contains helpers for rendering templates. These helpers can
// be registerd with the Handlebars library to assist in manipulating
// text at render time.

use handlebars::{Context, Handlebars, Helper, Output, RenderContext, RenderError};
use snafu::{OptionExt, ResultExt};

/// Potential errors during helper execution
mod error {
    use handlebars::RenderError;
    use snafu::Snafu;

    #[derive(Debug, Snafu)]
    #[snafu(visibility = "pub(super)")]
    pub(super) enum TemplateHelperError {
        #[snafu(display(
            "Incorrect number of params provided to helper '{}' in template '{}' - {} expected, {} received",
            helper,
            template,
            expected,
            received,
        ))]
        IncorrectNumberOfParams {
            expected: u8,
            received: usize,
            helper: String,
            template: String,
        },

        #[snafu(display("Internal error: {}", msg))]
        Internal { msg: String },

        // handlebars::JsonValue is a serde_json::Value, which implements
        // the 'Display' trait and should provide valuable context
        #[snafu(display(
            "Invalid (non-string) base64 template value: '{}' in template {}",
            value,
            template
        ))]
        InvalidTemplateValue {
            value: handlebars::JsonValue,
            template: String,
        },

        #[snafu(display(
            "Unable to base64 decode string '{}' in template '{}': '{}'",
            base64_string,
            template,
            source
        ))]
        Base64Decode {
            base64_string: String,
            template: String,
            source: base64::DecodeError,
        },

        #[snafu(display(
            "Invalid (non-utf8) output from base64 string '{}' in template '{}': '{}'",
            base64_string,
            template,
            source
        ))]
        InvalidUTF8 {
            base64_string: String,
            template: String,
            source: std::str::Utf8Error,
        },

        #[snafu(display("Unable to write template '{}': '{}'", template, source))]
        TemplateWrite {
            template: String,
            source: std::io::Error,
        },
    }

    // Handlebars helpers are required to return a RenderError.
    // Implement "From" for TemplateHelperError.
    impl From<TemplateHelperError> for RenderError {
        fn from(e: TemplateHelperError) -> RenderError {
            RenderError::with(e)
        }
    }
}

/// `base64_decode` decodes base64 encoded text at template render time.
/// It takes a single variable as a parameter: {{base64_decode var}}
pub fn base64_decode(
    helper: &Helper,
    _: &Handlebars,
    _: &Context,
    renderctx: &mut RenderContext,
    out: &mut Output,
) -> Result<(), RenderError> {
    // To give context to our errors, get the template name.
    // In the context of thar-be-settings, all of our templates have
    // registered names, which means that the get_root_template_name()
    // call should return an intelligible and valid name.
    // To protect us in the unlikely event a template was registered
    // without a name, we return the text "dynamic template"
    trace!("Starting base64_decode helper");
    let template_name = renderctx
        .get_root_template_name()
        .map(|i| i.to_string())
        .unwrap_or_else(|| "dynamic template".to_string());
    trace!("Template name: {}", &template_name);

    // Check number of parameters, must be exactly one
    trace!("Number of params: {}", helper.params().len());

    if helper.params().len() != 1 {
        return Err(RenderError::from(
            error::TemplateHelperError::IncorrectNumberOfParams {
                expected: 1,
                received: helper.params().len(),
                helper: helper.name().to_string(),
                template: template_name,
            },
        ));
    }

    // Get the resolved key out of the template (param(0)). value() returns
    // a serde_json::Value
    let base64_value = helper
        .param(0)
        .map(|v| v.value())
        .context(error::Internal {
            msg: "Found no params after confirming there is one param",
        })?;
    trace!("Base64 value from template: {}", base64_value);

    // Create an &str from the serde_json::Value
    let base64_str = base64_value.as_str().context(error::InvalidTemplateValue {
        value: base64_value.to_owned(),
        template: template_name.to_owned(),
    })?;
    trace!("Base64 string from template: {}", base64_str);

    // Base64 decode the &str
    let decoded_bytes = base64::decode(&base64_str).context(error::Base64Decode {
        base64_string: base64_str.to_string(),
        template: template_name.to_owned(),
    })?;

    // Create a valid utf8 str
    let decoded = std::str::from_utf8(&decoded_bytes).context(error::InvalidUTF8 {
        base64_string: base64_str.to_string(),
        template: template_name.to_owned(),
    })?;
    trace!("Decoded base64: {}", decoded);

    // Write the string out to the template
    out.write(decoded).context(error::TemplateWrite {
        template: template_name.to_owned(),
    })?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use handlebars::TemplateRenderError;
    use serde::Serialize;
    use serde_json::json;

    // A thin wrapper around the handlebars render_template method that includes
    // setup and registration of helpers
    fn setup_and_render_template<T>(tmpl: &str, data: &T) -> Result<String, TemplateRenderError>
    where
        T: Serialize,
    {
        let mut registry = Handlebars::new();
        registry.register_helper("base64_decode", Box::new(base64_decode));

        registry.render_template(tmpl, data)
    }

    #[test]
    fn renders_decoded_base64() {
        let result =
            setup_and_render_template("{{base64_decode var}}", &json!({"var": "SGk="})).unwrap();
        assert_eq!(result, "Hi")
    }

    #[test]
    fn does_not_render_invalid_base64() {
        assert!(setup_and_render_template("{{base64_decode var}}", &json!({"var": "hi"})).is_err())
    }

    #[test]
    fn does_not_render_invalid_utf8() {
        // "wygk" is the invalid UTF8 string "\xc3\x28" base64 encoded
        assert!(
            setup_and_render_template("{{base64_decode var}}", &json!({"var": "wygK"})).is_err()
        )
    }

    #[test]
    fn base64_helper_with_missing_param() {
        assert!(setup_and_render_template("{{base64_decode}}", &json!({"var": "foo"})).is_err());
    }

    #[test]
    fn base64_helper_with_extra_param() {
        assert!(setup_and_render_template(
            "{{base64_decode var1 var2}}",
            &json!({"var1": "Zm9v", "var2": "YmFy"})
        )
        .is_err());
    }
}
