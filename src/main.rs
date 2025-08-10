use std::sync::Arc;

use actix_web::{test, web, App, HttpServer};
use clap::Parser;
use env_logger::Env;
use log::{error, info};
use std::fs;

use libsubconverter::settings::settings::settings_struct::init_settings;
use libsubconverter::{web_handlers, Settings};

/// A more powerful utility to convert between proxy subscription format
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(short, long, value_name = "FILE")]
    config: Option<String>,

    /// Listen address (e.g., 127.0.0.1 or 0.0.0.0)
    #[arg(short, long, value_name = "ADDRESS")]
    address: Option<String>,

    /// Generate configurations locally using generate.ini
    #[arg(short = 'g', long = "generate")]
    generate: bool,
    
    /// Specify which artifact/task to process from generate.ini
    #[arg(long, value_name = "ARTIFACT_NAME")]
    artifact: Option<String>,
    
    /// Listen port
    #[arg(short, long, value_name = "PORT")]
    port: Option<u32>,

    /// Subscription URL to process directly instead of starting the server
    #[arg(long, value_name = "URL")]
    url: Option<String>,

    /// Output file path for subscription conversion (must be used with --url)
    #[arg(short, long, value_name = "OUTPUT_FILE")]
    output: Option<String>,
}

async fn handle_generate_mode(artifact_filter: Option<String>) -> std::io::Result<()> {
    use libsubconverter::utils::ini_reader::IniReader;
    
    // Read generate.ini file
    let mut ini_reader = IniReader::new();
    ini_reader.parse_file("generate.ini").await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, 
                                        format!("Failed to read generate.ini: {}", e)))?;
    
    // Get all section names
    let all_sections = ini_reader.get_section_names();
    
    // Filter sections based on artifact parameter
    let sections_to_process: Vec<&String> = if let Some(ref artifact) = artifact_filter {
        all_sections.iter()
            .filter(|&section_name| section_name == artifact)
            .collect()
    } else {
        all_sections.iter().collect()
    };
    
    if sections_to_process.is_empty() {
        if let Some(artifact) = artifact_filter {
            eprintln!("Error: Artifact '{}' not found in generate.ini", artifact);
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, 
                                          format!("Artifact '{}' not found", artifact)));
        } else {
            eprintln!("Warning: No sections found in generate.ini");
            return Ok(());
        }
    }
    
    info!("Processing {} section(s)", sections_to_process.len());
    
    // Process each selected section
    for section_name in sections_to_process {
        if section_name.is_empty() {
            continue;
        }
        
        info!("Processing artifact: [{}]", section_name);
        
        // Enter the section to read its contents
        if let Err(e) = ini_reader.enter_section(section_name) {
            error!("Failed to enter section {}: {:?}", section_name, e);
            continue;
        }
        
        // Extract parameters from section
        let path = ini_reader.get_current("path");
        let url = ini_reader.get_current("url");
        let target = ini_reader.get_current("target");
        
        if path.is_empty() || url.is_empty() || target.is_empty() {
            eprintln!("Skipping section [{}]: missing required parameters (path, url, target)", section_name);
            continue;
        }
        
        // Build query parameters
        let mut query_string = format!("target={}&url={}", 
                                      urlencoding::encode(&target),
                                      urlencoding::encode(&url));
        
        // Add optional parameters
        let ver = ini_reader.get_current("ver");
        if !ver.is_empty() {
            query_string.push_str(&format!("&ver={}", ver));
        }
        
        let emoji = ini_reader.get_current("emoji");
        if !emoji.is_empty() {
            query_string.push_str(&format!("&emoji={}", emoji));
        }
        
        let list = ini_reader.get_current("list");
        if !list.is_empty() {
            query_string.push_str(&format!("&list={}", list));
        }
        
        let include = ini_reader.get_current("include");
        if !include.is_empty() {
            query_string.push_str(&format!("&include={}", urlencoding::encode(&include)));
        }
        
        let exclude = ini_reader.get_current("exclude");
        if !exclude.is_empty() {
            query_string.push_str(&format!("&exclude={}", urlencoding::encode(&exclude)));
        }
        
        // Create test app and process
        let app = test::init_service(App::new().configure(web_handlers::config)).await;
        let req = test::TestRequest::get()
            .uri(&format!("/sub?{}", query_string))
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        
        if resp.status().is_success() {
            let body = test::read_body(resp).await;
            fs::write(&path, body)?;
            info!("Generated config file for [{}]: {}", section_name, path);
        } else {
            error!("Failed to generate config for section [{}]: {}", 
                   section_name, resp.status());
        }
    }
    
    Ok(())
}


#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize the logger
    env_logger::init_from_env(Env::default().default_filter_or("info"));

    // Parse command line arguments
    let args = Args::parse();

    // Check if only one of url or output is provided
    if args.url.is_some() != args.output.is_some() {
        eprintln!("Error: --url and -o/--output must be used together");
        std::process::exit(1);
    }

    // Check if generate mode is enabled first
    if args.generate {
        return handle_generate_mode(args.artifact).await;
    }
    
    // Initialize settings with config file path if provided
    init_settings(args.config.as_deref().unwrap_or(""))
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    
    // Check if URL is provided for direct processing
    if let Some(url) = args.url {
        let output_file = args
            .output
            .as_ref()
            .expect("Output file must be provided with URL");
        info!(
            "Processing subscription from URL: {} to file: {}",
            url, output_file
        );

        // Create a test app with the same configuration as the web app
        let app = test::init_service(App::new().configure(web_handlers::config)).await;

        // Create a test request with the correct URI
        let req = test::TestRequest::get().uri(&url).to_request();

        // Execute the request
        let resp = test::call_service(&app, req).await;

        
        
        // Check if the response is successful
        if resp.status().is_success() {
            // Get response body
            let body = test::read_body(resp).await;

            // Write the response to the output file
            fs::write(output_file, body)?;
            info!("Successfully wrote result to {}", output_file);
        } else {
            error!("API request failed with status: {}", resp.status());
            std::process::exit(1);
        }

        Ok(()) // Exit after processing the URL
    } else {
        // Proceed with starting the web server
        // Ensure we have a valid listen address
        let listen_address = {
            // Get a mutable reference to the current settings
            let mut settings_guard = Settings::current_mut();
            let settings = Arc::make_mut(&mut *settings_guard);

            // Override settings with command line arguments if provided
            if let Some(address) = args.address {
                settings.listen_address = address;
            }
            if let Some(port) = args.port {
                settings.listen_port = port;
            }
            if settings.listen_address.trim().is_empty() {
                error!("Empty listen_address in settings, defaulting to 127.0.0.1");
                format!("127.0.0.1:{}", settings.listen_port)
            } else {
                // Check if the address contains a port
                if settings.listen_address.contains(':') {
                    // Already has a port, use as is
                    settings.listen_address.clone()
                } else {
                    // No port specified, use the one from settings
                    format!("{}:{}", settings.listen_address, settings.listen_port)
                }
            }
        };

        let max_concur_threads = Settings::current().max_concur_threads;

        info!("Subconverter starting on {}", listen_address);

        // Start web server
        HttpServer::new(move || {
            App::new()
                // Register web handlers
                .configure(web_handlers::config)
                // For health check
                .route("/", web::get().to(|| async { "Subconverter is running!" }))
        })
        .bind(listen_address)?
        .workers(max_concur_threads as usize)
        .run()
        .await
    }
        }            }
            if settings.listen_address.trim().is_empty() {
                error!("Empty listen_address in settings, defaulting to 127.0.0.1");
                format!("127.0.0.1:{}", settings.listen_port)
            } else {
                // Check if the address contains a port
                if settings.listen_address.contains(':') {
                    // Already has a port, use as is
                    settings.listen_address.clone()
                } else {
                    // No port specified, use the one from settings
                    format!("{}:{}", settings.listen_address, settings.listen_port)
                }
            }
        };

        let max_concur_threads = Settings::current().max_concur_threads;

        info!("Subconverter starting on {}", listen_address);

        // Start web server
        HttpServer::new(move || {
            App::new()
                // Register web handlers
                .configure(web_handlers::config)
                // For health check
                .route("/", web::get().to(|| async { "Subconverter is running!" }))
        })
        .bind(listen_address)?
        .workers(max_concur_threads as usize)
        .run()
        .await
    }
}
