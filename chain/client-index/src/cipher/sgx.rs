#![allow(missing_docs)]
// Copyright 2019 MesaTEE Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// Modifications Copyright (c) 2019, Foris Limited (licensed under the Apache License, Version 2.0)
// TODO: document the SGX stuff
use chrono::DateTime;
use client_common::error::{Error, ErrorKind, Result};
use failure::ResultExt;
use rustls;
use serde_json;
use serde_json::Value;
use std::convert::TryFrom;
use std::io::BufReader;
use std::time::Duration;
use std::time::SystemTime;
use uuid::Uuid;

// TODO: do they all need to be supported?
type SignatureAlgorithms = &'static [&'static webpki::SignatureAlgorithm];
static SUPPORTED_SIG_ALGS: SignatureAlgorithms = &[
    &webpki::ECDSA_P256_SHA256,
    &webpki::ECDSA_P256_SHA384,
    &webpki::ECDSA_P384_SHA256,
    &webpki::ECDSA_P384_SHA384,
    &webpki::RSA_PSS_2048_8192_SHA256_LEGACY_KEY,
    &webpki::RSA_PSS_2048_8192_SHA384_LEGACY_KEY,
    &webpki::RSA_PSS_2048_8192_SHA512_LEGACY_KEY,
    &webpki::RSA_PKCS1_2048_8192_SHA256,
    &webpki::RSA_PKCS1_2048_8192_SHA384,
    &webpki::RSA_PKCS1_2048_8192_SHA512,
    &webpki::RSA_PKCS1_3072_8192_SHA384,
];

#[derive(PartialEq, Debug)]
pub enum SgxQuoteStatus {
    OK,
    GroupOutOfDate,
    ConfigurationNeeded,
    UnknownBadStatus,
}

impl From<&str> for SgxQuoteStatus {
    fn from(status: &str) -> Self {
        match status {
            "OK" => SgxQuoteStatus::OK,
            "GROUP_OUT_OF_DATE" => SgxQuoteStatus::GroupOutOfDate,
            "CONFIGURATION_NEEDED" => SgxQuoteStatus::ConfigurationNeeded,
            _ => SgxQuoteStatus::UnknownBadStatus,
        }
    }
}

pub enum SgxQuoteSigType {
    Unlinkable,
    Linkable,
}

pub struct SgxReport {
    pub cpu_svn: [u8; 16],
    pub misc_select: u32,
    pub attributes: [u8; 16],
    pub mr_enclave: [u8; 32],
    pub mr_signer: [u8; 32],
    pub isv_prod_id: u16,
    pub isv_svn: u16,
    pub report_data: [u8; 64],
}

pub enum SgxQuoteVersion {
    V1,
    V2,
}

pub struct SgxQuoteBody {
    pub version: SgxQuoteVersion,
    pub signature_type: SgxQuoteSigType,
    pub gid: u32,
    pub isv_svn_qe: u16,
    pub isv_svn_pce: u16,
    pub qe_vendor_id: Uuid,
    pub user_data: [u8; 20],
    pub report_body: SgxReport,
}

impl SgxQuoteBody {
    // TODO: A Result should be returned instead of Option
    fn parse_from<'a>(bytes: &'a [u8]) -> Option<Self> {
        let mut pos: usize = 0;
        // TODO: It is really unnecessary to construct a Vec<u8> each time.
        // Try to optimize this.
        let mut take = |n: usize| -> Option<&'a [u8]> {
            if n > 0 && bytes.len() >= pos + n {
                let ret = Some(&bytes[pos..pos + n]);
                pos += n;
                ret
            } else {
                None
            }
        };

        // off 0, size 2
        let version = match u16::from_le_bytes(<[u8; 2]>::try_from(take(2)?).ok()?) {
            1 => SgxQuoteVersion::V1,
            2 => SgxQuoteVersion::V2,
            _ => return None,
        };

        // off 2, size 2
        let signature_type = match u16::from_le_bytes(<[u8; 2]>::try_from(take(2)?).ok()?) {
            0 => SgxQuoteSigType::Unlinkable,
            1 => SgxQuoteSigType::Linkable,
            _ => return None,
        };

        // off 4, size 4
        let gid = u32::from_le_bytes(<[u8; 4]>::try_from(take(4)?).ok()?);

        // off 8, size 2
        let isv_svn_qe = u16::from_le_bytes(<[u8; 2]>::try_from(take(2)?).ok()?);

        // off 10, size 2
        let isv_svn_pce = u16::from_le_bytes(<[u8; 2]>::try_from(take(2)?).ok()?);

        // off 12, size 16
        let qe_vendor_id_raw = <[u8; 16]>::try_from(take(16)?).ok()?;
        let qe_vendor_id = Uuid::from_slice(&qe_vendor_id_raw).ok()?;

        // off 28, size 20
        let user_data = <[u8; 20]>::try_from(take(20)?).ok()?;

        // off 48, size 16
        let cpu_svn = <[u8; 16]>::try_from(take(16)?).ok()?;

        // off 64, size 4
        let misc_select = u32::from_le_bytes(<[u8; 4]>::try_from(take(4)?).ok()?);

        // off 68, size 28
        let _reserved = take(28)?;

        // off 96, size 16
        let attributes = <[u8; 16]>::try_from(take(16)?).ok()?;

        // off 112, size 32
        let mr_enclave = <[u8; 32]>::try_from(take(32)?).ok()?;

        // off 144, size 32
        let _reserved = take(32)?;

        // off 176, size 32
        let mr_signer = <[u8; 32]>::try_from(take(32)?).ok()?;

        // off 208, size 96
        let _reserved = take(96)?;

        // off 304, size 2
        let isv_prod_id = u16::from_le_bytes(<[u8; 2]>::try_from(take(2)?).ok()?);

        // off 306, size 2
        let isv_svn = u16::from_le_bytes(<[u8; 2]>::try_from(take(2)?).ok()?);

        // off 308, size 60
        let _reserved = take(60)?;

        // off 368, size 64
        let mut report_data = [0u8; 64];
        let _report_data = take(64)?;
        let mut _it = _report_data.iter();
        for i in report_data.iter_mut() {
            *i = *_it.next()?;
        }

        if pos != bytes.len() {
            return None;
        }

        Some(Self {
            version,
            signature_type,
            gid,
            isv_svn_qe,
            isv_svn_pce,
            qe_vendor_id,
            user_data,
            report_body: SgxReport {
                cpu_svn,
                misc_select,
                attributes,
                mr_enclave,
                mr_signer,
                isv_prod_id,
                isv_svn,
                report_data,
            },
        })
    }
}

pub struct SgxQuote {
    pub freshness: Duration,
    pub status: SgxQuoteStatus,
    pub body: SgxQuoteBody,
}

fn extract_att_parts(payload: Vec<u8>) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    // Extract each field
    let mut iter = payload.split(|x| *x == 0x7C);
    let attn_report_raw = iter
        .next()
        .ok_or_else(|| Error::from(ErrorKind::InvalidCertFormat))?;
    let sig_raw = iter
        .next()
        .ok_or_else(|| Error::from(ErrorKind::InvalidCertFormat))?;
    let sig = base64::decode(&sig_raw).context(ErrorKind::InvalidCertFormat)?;
    let sig_cert_raw = iter
        .next()
        .ok_or_else(|| Error::from(ErrorKind::InvalidCertFormat))?;
    let sig_cert_dec = base64::decode_config(&sig_cert_raw, base64::STANDARD)
        .context(ErrorKind::InvalidCertFormat)?;
    Ok((attn_report_raw.to_vec(), sig, sig_cert_dec))
}

fn extract_quote_body(attn_report: Value) -> Result<(u64, SgxQuoteStatus, SgxQuoteBody)> {
    // TODO: reduce the boilerplate (for monadic operations)
    // 1. Check timestamp is within 24H (90day is recommended by Intel)
    let time = attn_report
        .get("timestamp")
        .and_then(|value| value.as_str())
        .ok_or_else(|| Error::from(ErrorKind::BadAttnReport))?;
    let time_fixed = String::from(time) + "+0000";
    let date_time = DateTime::parse_from_str(&time_fixed, "%Y-%m-%dT%H:%M:%S%.f%z")
        .context(ErrorKind::BadAttnReport)?;
    let ts = date_time.naive_utc();
    let now = DateTime::<chrono::offset::Utc>::from(SystemTime::now()).naive_utc();
    let quote_freshness =
        u64::try_from((now - ts).num_seconds()).context(ErrorKind::BadAttnReport)?;

    // 2. Get quote status
    let status_string = attn_report
        .get("isvEnclaveQuoteStatus")
        .and_then(|value| value.as_str())
        .ok_or_else(|| Error::from(ErrorKind::BadAttnReport))?;
    let quote_status = SgxQuoteStatus::from(status_string);

    // 3. Get quote body
    let quote_encoded = attn_report
        .get("isvEnclaveQuoteBody")
        .and_then(|value| value.as_str())
        .ok_or_else(|| Error::from(ErrorKind::BadAttnReport))?;
    let quote_raw =
        base64::decode(&(quote_encoded.as_bytes())).context(ErrorKind::BadAttnReport)?;
    let quote_body = SgxQuoteBody::parse_from(quote_raw.as_slice())
        .ok_or_else(|| Error::from(ErrorKind::BadAttnReport))?;

    Ok((quote_freshness, quote_status, quote_body))
}

/// seems the closure is needed for type inference
#[allow(clippy::redundant_closure)]
fn extract_sgx_quote_from_mra_cert(cert_der: &[u8]) -> Result<SgxQuote> {
    // Before we reach here, Webpki already verifed the cert is properly signed
    use super::cert::*;

    let x509 = yasna::parse_der(cert_der, |reader| X509::load(reader))
        .context(ErrorKind::InvalidCertFormat)?;

    let tbs_cert: <TbsCert as Asn1Ty>::ValueTy = x509.0;

    let pub_key: <PubKey as Asn1Ty>::ValueTy = ((((((tbs_cert.1).1).1).1).1).1).0;
    let pub_k = (pub_key.1).0;

    let sgx_ra_cert_ext: <SgxRaCertExt as Asn1Ty>::ValueTy = (((((((tbs_cert.1).1).1).1).1).1).1).0;

    let payload: Vec<u8> = ((sgx_ra_cert_ext.0).1).0;

    let (attn_report_raw, sig, sig_cert_dec) = extract_att_parts(payload)?;

    let sig_cert =
        webpki::EndEntityCert::from(&sig_cert_dec).context(ErrorKind::InvalidCertFormat)?;

    // Verify if the signing cert is issued by Intel CA
    // TODO: move to a file and include_bytes
    let ias_report_ca = "
    -----BEGIN CERTIFICATE-----
MIIFSzCCA7OgAwIBAgIJANEHdl0yo7CUMA0GCSqGSIb3DQEBCwUAMH4xCzAJBgNV
BAYTAlVTMQswCQYDVQQIDAJDQTEUMBIGA1UEBwwLU2FudGEgQ2xhcmExGjAYBgNV
BAoMEUludGVsIENvcnBvcmF0aW9uMTAwLgYDVQQDDCdJbnRlbCBTR1ggQXR0ZXN0
YXRpb24gUmVwb3J0IFNpZ25pbmcgQ0EwIBcNMTYxMTE0MTUzNzMxWhgPMjA0OTEy
MzEyMzU5NTlaMH4xCzAJBgNVBAYTAlVTMQswCQYDVQQIDAJDQTEUMBIGA1UEBwwL
U2FudGEgQ2xhcmExGjAYBgNVBAoMEUludGVsIENvcnBvcmF0aW9uMTAwLgYDVQQD
DCdJbnRlbCBTR1ggQXR0ZXN0YXRpb24gUmVwb3J0IFNpZ25pbmcgQ0EwggGiMA0G
CSqGSIb3DQEBAQUAA4IBjwAwggGKAoIBgQCfPGR+tXc8u1EtJzLA10Feu1Wg+p7e
LmSRmeaCHbkQ1TF3Nwl3RmpqXkeGzNLd69QUnWovYyVSndEMyYc3sHecGgfinEeh
rgBJSEdsSJ9FpaFdesjsxqzGRa20PYdnnfWcCTvFoulpbFR4VBuXnnVLVzkUvlXT
L/TAnd8nIZk0zZkFJ7P5LtePvykkar7LcSQO85wtcQe0R1Raf/sQ6wYKaKmFgCGe
NpEJUmg4ktal4qgIAxk+QHUxQE42sxViN5mqglB0QJdUot/o9a/V/mMeH8KvOAiQ
byinkNndn+Bgk5sSV5DFgF0DffVqmVMblt5p3jPtImzBIH0QQrXJq39AT8cRwP5H
afuVeLHcDsRp6hol4P+ZFIhu8mmbI1u0hH3W/0C2BuYXB5PC+5izFFh/nP0lc2Lf
6rELO9LZdnOhpL1ExFOq9H/B8tPQ84T3Sgb4nAifDabNt/zu6MmCGo5U8lwEFtGM
RoOaX4AS+909x00lYnmtwsDVWv9vBiJCXRsCAwEAAaOByTCBxjBgBgNVHR8EWTBX
MFWgU6BRhk9odHRwOi8vdHJ1c3RlZHNlcnZpY2VzLmludGVsLmNvbS9jb250ZW50
L0NSTC9TR1gvQXR0ZXN0YXRpb25SZXBvcnRTaWduaW5nQ0EuY3JsMB0GA1UdDgQW
BBR4Q3t2pn680K9+QjfrNXw7hwFRPDAfBgNVHSMEGDAWgBR4Q3t2pn680K9+Qjfr
NXw7hwFRPDAOBgNVHQ8BAf8EBAMCAQYwEgYDVR0TAQH/BAgwBgEB/wIBADANBgkq
hkiG9w0BAQsFAAOCAYEAeF8tYMXICvQqeXYQITkV2oLJsp6J4JAqJabHWxYJHGir
IEqucRiJSSx+HjIJEUVaj8E0QjEud6Y5lNmXlcjqRXaCPOqK0eGRz6hi+ripMtPZ
sFNaBwLQVV905SDjAzDzNIDnrcnXyB4gcDFCvwDFKKgLRjOB/WAqgscDUoGq5ZVi
zLUzTqiQPmULAQaB9c6Oti6snEFJiCQ67JLyW/E83/frzCmO5Ru6WjU4tmsmy8Ra
Ud4APK0wZTGtfPXU7w+IBdG5Ez0kE1qzxGQaL4gINJ1zMyleDnbuS8UicjJijvqA
152Sq049ESDz+1rRGc2NVEqh1KaGXmtXvqxXcTB+Ljy5Bw2ke0v8iGngFBPqCTVB
3op5KBG3RjbF6RRSzwzuWfL7QErNC8WEy5yDVARzTA5+xmBc388v9Dm21HGfcC8O
DD+gT9sSpssq0ascmvH49MOgjt1yoysLtdCtJW/9FZpoOypaHx0R+mJTLwPXVMrv
DaVzWh5aiEx+idkSGMnX
-----END CERTIFICATE-----
    "
    .to_owned()
    .into_bytes();
    let mut ias_ca_stripped: Vec<u8> = ias_report_ca.clone();
    ias_ca_stripped.retain(|&x| x != 0x0d && x != 0x0a);
    let head_len = "-----BEGIN CERTIFICATE-----".len();
    let tail_len = "-----END CERTIFICATE-----".len();
    let full_len = ias_ca_stripped.len();
    let ias_ca_core: &[u8] = &ias_ca_stripped[head_len..full_len - tail_len];
    let ias_cert_dec = base64::decode_config(ias_ca_core, base64::STANDARD)
        .context(ErrorKind::InvalidCertFormat)?;
    let ias_cert_input = &ias_cert_dec;

    let mut ca_reader = BufReader::new(&ias_report_ca[..]);

    let mut root_store = rustls::RootCertStore::empty();
    // this should not fail
    root_store
        .add_pem_file(&mut ca_reader)
        .expect("Failed to add CA");

    let trust_anchors: Vec<webpki::TrustAnchor> = root_store
        .roots
        .iter()
        .map(|cert| cert.to_trust_anchor())
        .collect();

    let now_func = webpki::Time::try_from(SystemTime::now())
        .map_err(|_| Error::from(ErrorKind::WebpkiFailure))?;

    let chain = vec![ias_cert_input.as_slice()];

    sig_cert
        .verify_is_valid_tls_server_cert(
            SUPPORTED_SIG_ALGS,
            &webpki::TLSServerTrustAnchors(&trust_anchors),
            chain.as_slice(),
            now_func,
        )
        .context(ErrorKind::WebpkiFailure)?;

    // Verify the signature against the signing cert
    sig_cert
        .verify_signature(&webpki::RSA_PKCS1_2048_8192_SHA256, &attn_report_raw, &sig)
        .context(ErrorKind::WebpkiFailure)?;

    // Verify attestation report and extract quote body
    let attn_report: Value =
        serde_json::from_slice(&attn_report_raw).context(ErrorKind::BadAttnReport)?;

    let (quote_freshness, quote_status, quote_body) = extract_quote_body(attn_report)?;
    let raw_pub_k = pub_k.to_bytes();

    // According to RFC 5480 `Elliptic Curve Cryptography Subject Public Key Information',
    // SEC 2.2:
    // ``The first octet of the OCTET STRING indicates whether the key is
    // compressed or uncompressed.  The uncompressed form is indicated
    // by 0x04 and the compressed form is indicated by either 0x02 or
    // 0x03 (see 2.3.3 in [SEC1]).  The public key MUST be rejected if
    // any other value is included in the first octet.''
    //
    // Here, only the uncompressed form is allowed.
    let is_uncompressed = raw_pub_k[0] == 4;
    let pub_k = &raw_pub_k.as_slice()[1..];
    if !is_uncompressed || pub_k != &quote_body.report_body.report_data[..] {
        return Err(ErrorKind::BadAttnReport.into());
    }

    Ok(SgxQuote {
        freshness: std::time::Duration::from_secs(quote_freshness),
        status: quote_status,
        body: quote_body,
    })
}

#[derive(Clone)]
pub struct EnclaveAttr {}

impl EnclaveAttr {
    fn check_quote(&self, _quote: &SgxQuote) -> bool {
        // FIXME: check quote.body.report_body.mr_signer matches the expected one etc.
        true
    }

    fn check_in_cert_quote(&self, cert_der: &[u8]) -> bool {
        let quote_result = extract_sgx_quote_from_mra_cert(&cert_der);
        let quote: SgxQuote = match quote_result {
            Err(_) => {
                return false;
            }
            Ok(quote) => quote,
        };
        self.check_quote(&quote)
    }
}

impl rustls::ServerCertVerifier for EnclaveAttr {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        certs: &[rustls::Certificate],
        _hostname: webpki::DNSNameRef,
        _ocsp: &[u8],
    ) -> std::result::Result<rustls::ServerCertVerified, rustls::TLSError> {
        // This call automatically verifies certificate signature
        if certs.len() != 1 {
            return Err(rustls::TLSError::NoCertificatesPresented);
        }
        if self.check_in_cert_quote(&certs[0].0) {
            Ok(rustls::ServerCertVerified::assertion())
        } else {
            Err(rustls::TLSError::WebPKIError(
                webpki::Error::ExtensionValueInvalid,
            ))
        }
    }
}
