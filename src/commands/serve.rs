static LOCK: once_cell::sync::Lazy<async_lock::RwLock<()>> =
    once_cell::sync::Lazy::new(|| async_lock::RwLock::new(()));

async fn serve_file(config: &mut fpm::Config, path: &camino::Utf8Path) -> fpm::http::Response {
    let f = match config.get_file_and_package_by_id(path.as_str()).await {
        Ok(f) => f,
        Err(e) => {
            return fpm::server_error!("FPM-Error: path: {}, {:?}", path, e);
        }
    };

    // Auth Stuff
    if !f.is_static() {
        let req = if let Some(ref r) = config.request {
            r
        } else {
            return fpm::server_error!("request not set");
        };
        match config.can_read(req, path.as_str()).await {
            Ok(can_read) => {
                if !can_read {
                    return fpm::unauthorised!("You are unauthorized to access: {}", path);
                }
            }
            Err(e) => {
                return fpm::server_error!("FPM-Error: can_read error: {}, {:?}", path, e);
            }
        }
    }

    config.current_document = Some(f.get_id());
    match f {
        fpm::File::Ftd(main_document) => {
            match fpm::package_doc::read_ftd(config, &main_document, "/", false).await {
                Ok(r) => fpm::http::ok(r),
                Err(e) => {
                    fpm::server_error!("FPM-Error: path: {}, {:?}", path, e)
                }
            }
        }
        fpm::File::Image(image) => {
            fpm::http::ok_with_content_type(image.content, guess_mime_type(image.id.as_str()))
        }
        fpm::File::Static(s) => fpm::http::ok(s.content),
        _ => {
            fpm::server_error!("unknown handler")
        }
    }
}

async fn serve_cr_file(
    req: &fpm::http::Request,
    config: &mut fpm::Config,
    path: &camino::Utf8Path,
    cr_number: usize,
) -> fpm::http::Response {
    let _lock = LOCK.read().await;
    let f = match config
        .get_file_and_package_by_cr_id(path.as_str(), cr_number)
        .await
    {
        Ok(f) => f,
        Err(e) => {
            return fpm::server_error!("FPM-Error: path: {}, {:?}", path, e);
        }
    };

    // Auth Stuff
    if !f.is_static() {
        match config.can_read(req, path.as_str()).await {
            Ok(can_read) => {
                if !can_read {
                    return fpm::unauthorised!("You are unauthorized to access: {}", path);
                }
            }
            Err(e) => {
                return fpm::server_error!("FPM-Error: can_read error: {}, {:?}", path, e);
            }
        }
    }

    config.current_document = Some(f.get_id());
    match f {
        fpm::File::Ftd(main_document) => {
            match fpm::package_doc::read_ftd(config, &main_document, "/", false).await {
                Ok(r) => fpm::http::ok(r),
                Err(e) => {
                    fpm::server_error!("FPM-Error: path: {}, {:?}", path, e)
                }
            }
        }
        fpm::File::Image(image) => {
            fpm::http::ok_with_content_type(image.content, guess_mime_type(image.id.as_str()))
        }
        fpm::File::Static(s) => fpm::http::ok(s.content),
        _ => {
            fpm::server_error!("FPM unknown handler")
        }
    }
}

fn guess_mime_type(path: &str) -> mime_guess::Mime {
    mime_guess::from_path(path).first_or_octet_stream()
}

async fn serve_fpm_file(config: &fpm::Config) -> fpm::http::Response {
    let response =
        match tokio::fs::read(config.get_root_for_package(&config.package).join("FPM.ftd")).await {
            Ok(res) => res,
            Err(e) => {
                return fpm::not_found!("FPM-Error: path: FPM.ftd error: {:?}", e);
            }
        };
    fpm::http::ok_with_content_type(response, "application/octet-stream")
}

async fn static_file(file_path: camino::Utf8PathBuf) -> fpm::http::Response {
    if !file_path.exists() {
        return fpm::not_found!("no such static file ({})", file_path);
    }

    match tokio::fs::read(file_path.as_path()).await {
        Ok(r) => fpm::http::ok_with_content_type(r, guess_mime_type(file_path.as_str())),
        Err(e) => {
            fpm::not_found!("FPM-Error: path: {:?}, error: {:?}", file_path, e)
        }
    }
}

async fn serve(
    req: actix_web::HttpRequest,
    body: actix_web::web::Bytes, // TODO: Not liking it, It should be fetched from request only :(
) -> fpm::Result<fpm::http::Response> {
    let req = fpm::http::Request::from_actix(req, body);

    let _lock = LOCK.read().await;
    let r = format!("{} {}", req.method(), req.path());
    let t = fpm::time(r.as_str());
    println!("{r} started");

    // TODO: remove unwrap
    let path: camino::Utf8PathBuf = req.url_data("path").parse().unwrap();

    let favicon = camino::Utf8PathBuf::new().join("favicon.ico");
    let response = if path.eq(&favicon) {
        static_file(favicon).await
    } else if path.eq(&camino::Utf8PathBuf::new().join("FPM.ftd")) {
        let config = fpm::time("Config::read()").it(fpm::Config::read(None, false).await.unwrap());
        serve_fpm_file(&config).await
    } else if path.eq(&camino::Utf8PathBuf::new().join("")) {
        let mut config = fpm::time("Config::read()")
            .it(fpm::Config::read(None, false).await.unwrap())
            .set_request(req);
        serve_file(&mut config, &path.join("/")).await
    } else if let Some(cr_number) = fpm::cr::get_cr_path_from_url(path.as_str()) {
        let mut config =
            fpm::time("Config::read()").it(fpm::Config::read(None, false).await.unwrap());
        serve_cr_file(&req, &mut config, &path, cr_number).await
    } else {
        // url is present in config or not
        // If not present than proxy pass it
        let mut config = fpm::time("Config::read()").it(fpm::Config::read(None, false)
            .await
            .unwrap()
            .set_request(req));

        // If path is not present in sitemap then pass it to proxy
        // TODO: Need to handle other package URL as well, and that will start from `-`
        // and all the static files starts with `-`
        // So task is how to handle proxy urls which are starting with `-/<package-name>/<proxy-path>`
        // In that case need to check whether that url is present in sitemap or not and than proxy
        // pass the url if the file is not static
        // So final check would be file is not static and path is not present in the package's sitemap
        if !path.starts_with("-/") {
            if let Some(sitemap) = config.package.sitemap.as_ref() {
                if !sitemap.path_exists(path.as_str()) {
                    if let Some(endpoint) = config.package.endpoint.as_ref() {
                        let req = if let Some(r) = config.request {
                            r
                        } else {
                            return Ok(fpm::server_error!("request not set"));
                        };

                        return fpm::proxy::get_out(endpoint, req).await;
                    };
                }
            }
        }
        // TODO: pass &fpm::http::Request

        serve_file(&mut config, &path).await

        // if true: serve_file
        // else: proxy_pass
    };
    t.it(Ok(response))
}

pub(crate) async fn download_init_package(url: Option<String>) -> std::io::Result<()> {
    let mut package = fpm::Package::new("unknown-package");
    package.download_base_url = url;
    package
        .http_download_by_id(
            "FPM.ftd",
            Some(
                &camino::Utf8PathBuf::from_path_buf(std::env::current_dir()?)
                    .expect("FPM-Error: Unable to change path"),
            ),
        )
        .await
        .expect("Unable to find FPM.ftd file");
    Ok(())
}

pub async fn clear_cache(
    req: actix_web::HttpRequest,
    body: actix_web::web::Bytes,
) -> fpm::http::Response {
    let _lock = LOCK.write().await;
    fpm::apis::cache::clear(&fpm::http::Request::from_actix(req, body)).await
}

// TODO: Move them to routes folder
async fn sync(
    req: actix_web::web::Json<fpm::apis::sync::SyncRequest>,
) -> fpm::Result<fpm::http::Response> {
    let _lock = LOCK.write().await;
    fpm::apis::sync(req.0).await
}

async fn sync2(
    req: actix_web::web::Json<fpm::apis::sync2::SyncRequest>,
) -> fpm::Result<fpm::http::Response> {
    let _lock = LOCK.write().await;
    fpm::apis::sync2(req.0).await
}

pub async fn clone() -> fpm::Result<fpm::http::Response> {
    let _lock = LOCK.read().await;
    fpm::apis::clone().await
}

pub(crate) async fn view_source(
    req: actix_web::HttpRequest,
    body: actix_web::web::Bytes,
) -> fpm::http::Response {
    let _lock = LOCK.read().await;
    fpm::apis::view_source(fpm::http::Request::from_actix(req, body)).await
}

pub async fn edit(
    req: actix_web::HttpRequest,
    req_data: actix_web::web::Json<fpm::apis::edit::EditRequest>,
    body: actix_web::web::Bytes,
) -> fpm::Result<fpm::http::Response> {
    let _lock = LOCK.write().await;
    fpm::apis::edit(fpm::http::Request::from_actix(req, body), req_data.0).await
}

pub async fn revert(
    req: actix_web::web::Json<fpm::apis::edit::RevertRequest>,
) -> fpm::Result<fpm::http::Response> {
    let _lock = LOCK.write().await;
    fpm::apis::edit::revert(req.0).await
}

pub async fn editor_sync() -> fpm::Result<fpm::http::Response> {
    let _lock = LOCK.write().await;
    fpm::apis::edit::sync().await
}

pub async fn create_cr(
    req: actix_web::web::Json<fpm::apis::cr::CreateCRRequest>,
) -> fpm::Result<fpm::http::Response> {
    let _lock = LOCK.write().await;
    fpm::apis::cr::create_cr(req.0).await
}

pub async fn create_cr_page() -> fpm::Result<fpm::http::Response> {
    let _lock = LOCK.read().await;
    fpm::apis::cr::create_cr_page().await
}

pub async fn listen(
    bind_address: &str,
    port: Option<u16>,
    package_download_base_url: Option<String>,
) -> std::io::Result<()> {
    use colored::Colorize;

    if package_download_base_url.is_some() {
        download_init_package(package_download_base_url).await?;
    }

    if cfg!(feature = "controller") {
        // fpm-controller base path and ec2 instance id (hardcoded for now)
        let fpm_controller: String = std::env::var("FPM_CONTROLLER")
            .unwrap_or_else(|_| "https://controller.fifthtry.com".to_string());
        let fpm_instance: String =
            std::env::var("FPM_INSTANCE_ID").expect("FPM_INSTANCE_ID is required");

        println!("Resolving dependency");
        match crate::controller::resolve_dependencies(fpm_instance, fpm_controller).await {
            Ok(_) => println!("Dependencies resolved"),
            Err(e) => panic!("Error resolving dependencies using controller!!: {:?}", e),
        }
    }

    fn get_available_port(port: Option<u16>, bind_address: &str) -> Option<std::net::TcpListener> {
        let available_listener =
            |port: u16, bind_address: &str| std::net::TcpListener::bind((bind_address, port));

        if let Some(port) = port {
            return match available_listener(port, bind_address) {
                Ok(l) => Some(l),
                Err(_) => None,
            };
        }

        for x in 8000..9000 {
            match available_listener(x, bind_address) {
                Ok(l) => return Some(l),
                Err(_) => continue,
            }
        }
        None
    }

    let tcp_listener = match get_available_port(port, bind_address) {
        Some(listener) => listener,
        None => {
            eprintln!(
                "{}",
                port.map(|x| format!(
                    r#"Provided port {} is not available.

You can try without providing port, it will automatically pick unused port."#,
                    x.to_string().red()
                ))
                .unwrap_or_else(|| {
                    "Tried picking port between port 8000 to 9000, none are available :-("
                        .to_string()
                })
            );
            std::process::exit(2);
        }
    };

    let app = move || {
        {
            if cfg!(feature = "remote") {
                let json_cfg = actix_web::web::JsonConfig::default()
                    .content_type(|mime| mime == mime_guess::mime::APPLICATION_JSON)
                    .limit(9862416400); // TODO: explain this number

                actix_web::App::new()
                    .app_data(json_cfg)
                    .route("/-/sync/", actix_web::web::post().to(sync))
                    .route("/-/sync2/", actix_web::web::post().to(sync2))
                    .route("/-/clone/", actix_web::web::get().to(clone))
            } else {
                actix_web::App::new()
            }
        }
        .route(
            "/-/view-src/{path:.*}",
            actix_web::web::get().to(view_source),
        )
        .route("/-/edit/", actix_web::web::post().to(edit))
        .route("/-/revert/", actix_web::web::post().to(revert))
        .route("/-/editor-sync/", actix_web::web::get().to(editor_sync))
        .route("/-/create-cr/", actix_web::web::post().to(create_cr))
        .route("/-/create-cr/", actix_web::web::get().to(create_cr_page))
        .route("/-/clear-cache/", actix_web::web::post().to(clear_cache))
        .route("/{path:.*}", actix_web::web::route().to(serve))
    };

    println!("### Server Started ###");
    println!(
        "Go to: http://{}:{}",
        bind_address,
        tcp_listener.local_addr()?.port()
    );
    actix_web::HttpServer::new(app)
        .listen(tcp_listener)?
        .run()
        .await
}

// cargo install --features controller --path=.
// FPM_CONTROLLER=http://127.0.0.1:8000 FPM_INSTANCE_ID=12345 fpm serve 8001
