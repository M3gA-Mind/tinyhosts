//! Loads a built module through the real `TinyBus` dynamic loader.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use tinybus::Connection;
use tinybus::broker::Broker;
use tinybus::module::ModuleHost;
use tinybus::transport::memory::MemoryBus;

const INTERFACE: &str = "ai.tinyhumans.tinyhosts.Hosting";
const OBJECT_PATH: &str = "/ai/tinyhumans/tinyhosts/Hosting";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let module = module_argument()?;
    let bus = MemoryBus::new();
    let broker = Broker::new();
    let broker_task = broker.spawn(bus.clone());
    let module_host = ModuleHost::new(broker);
    let info = module_host.load_file(&module)?;

    if info.name != env!("CARGO_PKG_NAME") {
        return Err(io::Error::other(format!(
            "loaded module `{}` instead of `{}`",
            info.name,
            env!("CARGO_PKG_NAME")
        ))
        .into());
    }

    let client = Connection::connect(bus.connect().await?).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let names = client.list_names().await?;
            if names.iter().any(|name| name.as_str() == INTERFACE) {
                return tinybus::Result::Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;

    let proxy = client.proxy(INTERFACE, OBJECT_PATH, INTERFACE)?;
    let providers: String = proxy.call("Providers", ()).await?;
    if !providers.contains("vercel") {
        return Err(
            io::Error::other(format!("module reported unexpected providers: {providers}")).into(),
        );
    }

    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/v10/projects"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"projects": [{"id": "fixture", "name": "fixture"}]}),
            ),
        )
        .mount(&server)
        .await;
    let operation: tinyhosts_bus::rpc::Operation =
        tinyhosts_bus::rpc::Operation::ListSites { limit: 20 };
    let mut request = serde_json::to_value(operation)?;
    request["credentials"] = serde_json::json!({"api_key":"local-fixture"});
    request["base_url"] = serde_json::json!(server.uri());
    let response: String = proxy.call("Execute", (request.to_string(),)).await?;
    let outcome: tinyhosts_bus::rpc::Outcome = serde_json::from_str(&response)?;
    match outcome {
        tinyhosts_bus::rpc::Outcome::Sites(sites)
            if sites.len() == 1 && sites[0].id == "fixture" => {}
        _ => {
            return Err(io::Error::other("compiled module did not return the fixture site").into());
        }
    }

    verify_preparation(&client, &server).await?;
    verify_legacy_large_deployment(&client, &server).await?;

    println!(
        "verified {} as TinyBus module `{}`",
        module.display(),
        info.name
    );
    broker_task.abort();
    Ok(())
}

fn module_argument() -> Result<PathBuf, io::Error> {
    std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: cargo run --example verify_module -- <module-path>",
            )
        })
}

async fn verify_preparation(
    client: &Connection,
    server: &wiremock::MockServer,
) -> Result<(), Box<dyn std::error::Error>> {
    let proxy = client.proxy(INTERFACE, OBJECT_PATH, INTERFACE)?;
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("index.html"), "Hello")?;
    let prepared: String = proxy
        .call(
            "Execute",
            (serde_json::json!({
                "operation":"prepare_bundle", "directory":{
                    "workspace": directory.path().canonicalize()?.to_string_lossy(), "path":"."
                }
            })
            .to_string(),),
        )
        .await?;
    let prepared: tinyhosts_bus::rpc::Outcome = serde_json::from_str(&prepared)?;
    let tinyhosts_bus::rpc::Outcome::PreparedBundle(snapshot) = prepared else {
        return Err(io::Error::other("missing prepared snapshot").into());
    };
    if snapshot.contract_version != (1, 1) || snapshot.total_bytes != 5 {
        return Err(io::Error::other("incorrect preparation facts").into());
    }
    std::fs::write(
        directory.path().join("index.html"),
        "Changed after preparation",
    )?;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v2/files"))
        .and(wiremock::matchers::body_string("Hello"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v13/deployments"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({
            "files":[{"file":"index.html", "size":5}]
        })))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"id":"snapshot-deploy","readyState":"READY"})),
        )
        .expect(1)
        .mount(server)
        .await;
    let deployed: String = proxy
        .call(
            "Execute",
            (serde_json::json!({
                "operation":"deploy", "credentials":{"api_key":"local-fixture"},
                "base_url":server.uri(), "request":{"site":"fixture", "bundle":snapshot.bundle}
            })
            .to_string(),),
        )
        .await?;
    if !deployed.contains("snapshot-deploy") {
        return Err(io::Error::other("snapshot deployment did not use captured bytes").into());
    }
    server.verify().await;

    Ok(())
}

async fn verify_legacy_large_deployment(
    client: &Connection,
    server: &wiremock::MockServer,
) -> Result<(), Box<dyn std::error::Error>> {
    use base64::Engine as _;
    let bytes = vec![b'x'; 6 * 1024 * 1024];
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v2/files"))
        .and(wiremock::matchers::body_bytes(bytes.clone()))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v13/deployments"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({
            "files":[{"file":"large.html", "size":bytes.len()}]
        })))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"id":"large-deploy","readyState":"READY"})),
        )
        .expect(1)
        .mount(server)
        .await;
    let request = serde_json::json!({
        "operation":"deploy", "credentials":{"api_key":"local-fixture"},
        "base_url":server.uri(), "request":{"site":"fixture", "bundle":[{
            "path":"large.html", "contents":base64::engine::general_purpose::STANDARD.encode(bytes)
        }]}
    })
    .to_string();
    if request.len() <= 7 * 1024 * 1024
        || serde_json::to_vec(&request)?.len() >= 16 * 1024 * 1024 - 1024
    {
        return Err(
            io::Error::other("large legacy fixture is outside intended wire budget").into(),
        );
    }
    let proxy = client.proxy(INTERFACE, OBJECT_PATH, INTERFACE)?;
    let deployed: String = proxy.call("Execute", (request,)).await?;
    if !deployed.contains("large-deploy") {
        return Err(io::Error::other("large legacy deployment failed").into());
    }
    server.verify().await;
    Ok(())
}
