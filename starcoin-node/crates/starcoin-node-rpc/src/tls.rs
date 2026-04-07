use super::*;

pub(crate) fn build_http_client(
    config: &RuntimeConfig,
    default_headers: HeaderMap,
) -> anyhow::Result<(Client, Url)> {
    let mut endpoint = config.rpc_endpoint_url.clone();
    let mut builder = Client::builder()
        .connect_timeout(config.connect_timeout)
        .timeout(config.request_timeout)
        .default_headers(default_headers);

    let tls_server_name = config
        .tls_server_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if (tls_server_name.is_some() || !config.tls_pinned_spki_sha256.is_empty())
        && endpoint.scheme() != "https"
    {
        return Err(anyhow!(
            "tls_server_name and tls_pinned_spki_sha256 require an https rpc endpoint"
        ));
    }

    if endpoint.scheme() == "https" {
        if let Some(server_name) = tls_server_name {
            let original_host = endpoint
                .host_str()
                .context("rpc endpoint is missing host")?;
            if !original_host.eq_ignore_ascii_case(server_name) {
                let addrs = resolve_endpoint_addrs(&endpoint)?;
                endpoint
                    .set_host(Some(server_name))
                    .map_err(|_| anyhow!("invalid tls_server_name {server_name}"))?;
                builder = builder.resolve_to_addrs(server_name, &addrs);
            }
        }

        if tls_server_name.is_some()
            || !config.tls_pinned_spki_sha256.is_empty()
            || config.allow_insecure_remote_transport
        {
            builder = builder.use_preconfigured_tls(build_rustls_client_config(config)?);
        }
    }

    let http = builder.build().context("failed to build rpc http client")?;
    Ok((http, endpoint))
}

fn build_rustls_client_config(config: &RuntimeConfig) -> anyhow::Result<ClientConfig> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let supported_algorithms = provider.signature_verification_algorithms;
    let pins = parse_spki_pins(&config.tls_pinned_spki_sha256)?;
    let base_verifier = if config.allow_insecure_remote_transport {
        None
    } else {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let verifier: Arc<dyn ServerCertVerifier> =
            WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone())
                .build()
                .context("failed to build TLS verifier")?;
        Some(verifier)
    };
    let verifier = Arc::new(ConfiguredServerCertVerifier {
        base_verifier,
        pins,
        supported_algorithms,
    });
    let tls = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(DEFAULT_VERSIONS)
        .context("invalid TLS protocol version configuration")?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(tls)
}

fn parse_spki_pins(pins: &[String]) -> anyhow::Result<Vec<[u8; 32]>> {
    pins.iter().map(|pin| parse_spki_pin(pin)).collect()
}

fn parse_spki_pin(pin: &str) -> anyhow::Result<[u8; 32]> {
    let normalized = pin
        .trim()
        .strip_prefix("sha256/")
        .unwrap_or(pin.trim())
        .trim_start_matches("0x");

    if let Ok(bytes) = hex::decode(normalized) {
        if bytes.len() == 32 {
            return Ok(bytes
                .try_into()
                .expect("32-byte hex-encoded SPKI hash should fit"));
        }
    }

    let bytes = BASE64_STANDARD
        .decode(normalized)
        .with_context(|| format!("invalid tls_pinned_spki_sha256 value: {pin}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("tls_pinned_spki_sha256 entries must decode to 32 bytes"))
}

fn resolve_endpoint_addrs(endpoint: &Url) -> anyhow::Result<Vec<SocketAddr>> {
    let host = endpoint
        .host_str()
        .context("rpc endpoint is missing host")?;
    let port = endpoint
        .port_or_known_default()
        .context("rpc endpoint is missing a usable port")?;
    let addrs = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve rpc endpoint host {host}"))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(anyhow!(
            "resolved no socket addresses for rpc endpoint host {host}"
        ));
    }
    Ok(addrs)
}

#[derive(Debug)]
struct ConfiguredServerCertVerifier {
    base_verifier: Option<Arc<dyn ServerCertVerifier>>,
    pins: Vec<[u8; 32]>,
    supported_algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for ConfiguredServerCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        if let Some(base_verifier) = &self.base_verifier {
            base_verifier.verify_server_cert(
                end_entity,
                intermediates,
                server_name,
                ocsp_response,
                now,
            )?;
        }

        if !self.pins.is_empty() {
            let actual_pin = extract_spki_pin_from_certificate(end_entity)?;
            if !self.pins.iter().any(|expected| *expected == actual_pin) {
                return Err(TlsError::General(
                    "server certificate SPKI pin mismatch".to_owned(),
                ));
            }
        }

        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported_algorithms.supported_schemes()
    }
}

fn extract_spki_pin_from_certificate(cert: &CertificateDer<'_>) -> Result<[u8; 32], TlsError> {
    let (_, certificate) = X509Certificate::from_der(cert.as_ref()).map_err(|_| {
        TlsError::General("failed to parse server certificate for SPKI pinning".to_owned())
    })?;
    let digest = Sha256::digest(certificate.public_key().raw);
    let mut fingerprint = [0u8; 32];
    fingerprint.copy_from_slice(&digest);
    Ok(fingerprint)
}
