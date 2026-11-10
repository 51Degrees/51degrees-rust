/* *********************************************************************
 * This Original Work is copyright of 51 Degrees Mobile Experts Limited.
 * Copyright 2026 51 Degrees Mobile Experts Limited, Davidson House,
 * Forbury Square, Reading, Berkshire, United Kingdom RG1 3EU.
 *
 * This Original Work is licensed under the European Union Public Licence
 * (EUPL) v.1.2 and is subject to its terms as set out below.
 *
 * If a copy of the EUPL was not distributed with this file, You can obtain
 * one at https://opensource.org/licenses/EUPL-1.2.
 *
 * The 'Compatible Licences' set out in the Appendix to the EUPL (as may be
 * amended by the European Commission) shall be deemed incompatible for
 * the purposes of the Work and the provisions of the compatibility
 * clause in Article 5 of the EUPL shall not apply.
 *
 * If using the Work as, or as part of, a network application, by
 * including the attribution notice(s) required under Article 5 of the EUPL
 * in the end user terms of the application under an appropriate heading,
 * such notice(s) shall fulfill the requirements of that article.
 * ********************************************************************* */

//! A data update replaces the data file of a running engine under every
//! profile, and the copy the engine reads leaves with it. The test needs the
//! ASN file `_51DEGREES_IPI_PATH` names, and skips with a note without it.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fiftyone_ip_intelligence_onpremise::{IpIntelligenceOnPremiseEngine, IP_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::{AutoUpdateStatus, DataUpdateService, OnPremiseAspectEngine};

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Serve `body` to one request, and return the URL.
fn serve_once(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
    });
    (url, server)
}

/// With automatic updates on, an update replaces the data file of a running
/// engine under every profile, and the engine moves to the new file. No copy
/// is left once the engine is dropped.
#[test]
fn an_update_replaces_the_data_file_under_every_profile() {
    let Some(asn) = std::env::var("_51DEGREES_IPI_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    else {
        eprintln!("skipping operational copies: set _51DEGREES_IPI_PATH to the ASN file");
        return;
    };
    for profile in [
        PerformanceProfile::Default,
        PerformanceProfile::Balanced,
        PerformanceProfile::LowMemory,
        PerformanceProfile::HighPerformance,
        PerformanceProfile::InMemory,
    ] {
        let dir = std::env::temp_dir().join(format!(
            "fiftyone-ipi-copy-{}-{}",
            profile.as_str(),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let name = format!("copied-{}-{}.ipi", profile.as_str(), std::process::id());
        let path = dir.join(&name);
        std::fs::copy(&asn, &path).unwrap();
        // Compressed, as the Distributor serves a data file.
        let compressed = {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            encoder.write_all(&std::fs::read(&asn).unwrap()).unwrap();
            encoder.finish().unwrap()
        };
        let (url, server) = serve_once(compressed);
        let service = Arc::new(DataUpdateService::new());
        let engine = IpIntelligenceOnPremiseEngine::builder(&path)
            .performance_profile(profile)
            .properties(["Asn"])
            .auto_update(true)
            .file_system_watcher(false)
            .polling_interval_seconds(24 * 60 * 60)
            .max_randomisation_seconds(0)
            .data_update_url(url)
            .data_update_license_keys(["KEY"])
            .data_update_service(Arc::clone(&service))
            .build_shared()
            .expect("the data file loads");
        let as_engine: Arc<dyn OnPremiseAspectEngine> = engine.clone();

        let status = service.check_for_update(&as_engine, None);
        server.join().unwrap();

        assert_eq!(
            status.map_err(|error| error.to_string()),
            Ok(AutoUpdateStatus::Success),
            "{profile:?}"
        );
        let element: Arc<dyn FlowElement> = engine.clone();
        let pipeline = Pipeline::builder().add_element(element).build().unwrap();
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("server.client-ip", "1.1.1.1")
                .build(),
        );
        data.process().unwrap();
        assert!(data.get(IP_DATA_KEY).unwrap().string("Asn").value().is_ok());

        drop(data);
        drop(pipeline);
        drop(as_engine);
        drop(engine);
        drop(service);
        let left = files_in(&std::env::temp_dir())
            .into_iter()
            .filter(|file| file.starts_with(&name))
            .count();
        assert_eq!(left, 0, "{profile:?}: copies left in the temporary folder");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The copy is made in the folder the builder names, on build and on each
/// refresh, and none is left there once the engine is dropped.
#[test]
fn the_copy_goes_in_the_folder_the_builder_names() {
    let Some(asn) = std::env::var("_51DEGREES_IPI_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    else {
        eprintln!("skipping operational copies: set _51DEGREES_IPI_PATH to the ASN file");
        return;
    };
    let dir = std::env::temp_dir().join(format!("fiftyone-ipi-named-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let temp = dir.join("temp");
    std::fs::create_dir_all(&temp).unwrap();
    let path = dir.join("51Degrees.ipi");
    std::fs::copy(&asn, &path).unwrap();
    let copies = || files_in(&temp).len();

    let engine = IpIntelligenceOnPremiseEngine::builder(&path)
        .properties(["Asn"])
        .temp_dir_path(&temp)
        .auto_update(true)
        .file_system_watcher(false)
        .data_update_license_keys(["KEY"])
        .build()
        .expect("the data file loads");
    assert_eq!(copies(), 1, "the copy is in the named folder");

    engine.refresh(None).expect("the data file reloads");
    assert_eq!(copies(), 1, "the refresh copies into the named folder too");

    drop(engine);
    assert_eq!(copies(), 0, "no copy outlives the engine");
    let _ = std::fs::remove_dir_all(&dir);
}
