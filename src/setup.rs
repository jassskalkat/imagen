use std::io::{self, Write};
use std::path::PathBuf;

use crate::config::{config_file_path, default_output_dir, ConfigFile, Provider};

/// Run the one-time setup wizard that writes the local config file.
pub fn run() -> Result<(), String> {
    let path = config_file_path();
    let openai_org_id_default = std::env::var("OPENAI_ORG_ID").ok();
    let azure_endpoint_default = std::env::var("AZURE_OPENAI_ENDPOINT").ok();
    let azure_deployment_default = std::env::var("AZURE_OPENAI_DEPLOYMENT").ok();
    let azure_api_version_default = std::env::var("AZURE_OPENAI_API_VERSION").ok();

    println!("imagen setup");
    println!("This writes your local MCP config to: {}", path.display());

    if path.exists() && !prompt_yes_no("A config file already exists. Overwrite it?", false)? {
        println!("Setup cancelled.");
        return Ok(());
    }

    let provider = prompt_provider()?;
    let mut config = ConfigFile {
        provider: Some(provider.clone()),
        output_dir: Some(default_output_dir()),
        max_concurrent_jobs: Some(4),
        default_model: Some("gpt-image-2.5-sunburst".to_string()),
        ..Default::default()
    };

    match provider {
        Provider::OpenAI => {
            config.openai_api_key = Some(prompt_required("OpenAI API key")?);
            config.openai_org_id =
                prompt_optional("OpenAI organization ID", openai_org_id_default.as_deref())?;
        }
        Provider::Azure => {
            config.azure_api_key = Some(prompt_required("Azure OpenAI API key")?);
            let endpoint_or_resource = prompt_required_with_default(
                "Azure OpenAI resource name or endpoint",
                azure_endpoint_default.as_deref().unwrap_or("your-resource"),
            )?;
            config.azure_endpoint = Some(normalize_azure_endpoint(&endpoint_or_resource));
            config.azure_deployment_name = Some(
                azure_deployment_default.unwrap_or_else(|| "gpt-image-2.5-sunburst".to_string()),
            );
            config.azure_api_version =
                Some(azure_api_version_default.unwrap_or_else(|| "2025-04-01-preview".to_string()));
        }
    }

    config.save_to_path(&path).map_err(|e| e.to_string())?;

    println!();
    println!("Saved configuration.");
    println!("MCP entry for ~/.copilot/mcp-config.json:");
    println!(r#"  "imagen": {{"tools":["*"],"type":"local","command":"imagen"}}"#);
    println!();
    println!("If the binary is not on PATH, replace \"imagen\" with the full path.");
    println!("Artifacts will be written to: {}", default_output_dir());

    Ok(())
}

fn prompt_provider() -> Result<Provider, String> {
    loop {
        println!();
        println!("Choose a provider:");
        println!("  1) OpenAI");
        println!("  2) Azure OpenAI");
        let choice = prompt_with_default("Provider", "1")?;
        match choice.as_str() {
            "1" | "openai" => return Ok(Provider::OpenAI),
            "2" | "azure" => return Ok(Provider::Azure),
            other => {
                println!("Unknown choice: {other}");
            }
        }
    }
}

fn prompt_required(label: &str) -> Result<String, String> {
    loop {
        let value = prompt_raw(label, None)?;
        if !value.trim().is_empty() {
            return Ok(value.trim().to_string());
        }
        println!("{label} cannot be empty.");
    }
}

fn prompt_required_with_default(label: &str, default: &str) -> Result<String, String> {
    let value = prompt_raw(label, Some(default))?;
    if value.trim().is_empty() {
        Ok(default.to_string())
    } else {
        Ok(value.trim().to_string())
    }
}

fn prompt_optional(label: &str, default: Option<&str>) -> Result<Option<String>, String> {
    let value = prompt_raw(label, default)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Ok(default.map(|d| d.to_string()))
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn prompt_with_default(label: &str, default: &str) -> Result<String, String> {
    let value = prompt_raw(label, Some(default))?;
    if value.trim().is_empty() {
        Ok(default.to_string())
    } else {
        Ok(value.trim().to_string())
    }
}

fn prompt_yes_no(label: &str, default_yes: bool) -> Result<bool, String> {
    let default = if default_yes { "Y/n" } else { "y/N" };
    loop {
        let answer = prompt_raw(label, Some(default))?;
        match answer.trim().to_lowercase().as_str() {
            "" => return Ok(default_yes),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            other => println!("Please answer yes or no, not '{other}'."),
        }
    }
}

fn prompt_raw(label: &str, default: Option<&str>) -> Result<String, String> {
    let suffix = default
        .filter(|value| !value.is_empty())
        .map(|value| format!(" [{value}]"))
        .unwrap_or_default();
    print!("{label}{suffix}: ");
    io::stdout().flush().map_err(|e| e.to_string())?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| e.to_string())?;
    Ok(input)
}

fn normalize_azure_endpoint(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}.openai.azure.com")
    }
}

#[allow(dead_code)]
fn _config_path_for_docs() -> PathBuf {
    config_file_path()
}
