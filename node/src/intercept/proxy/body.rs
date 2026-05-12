use flate2::read::{DeflateDecoder, GzDecoder};
use std::io::{Cursor, Read};

pub fn decompress_grpc_payload(payload: &[u8]) -> Vec<u8> {
    if payload.len() < 5 {
        return payload.to_vec();
    }

    let compressed = payload[0] != 0;
    let message_len = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]) as usize;

    if payload.len() < 5 + message_len {
        return payload.to_vec();
    }

    let message_data = &payload[5..5 + message_len];

    if compressed {
        let mut decoder = GzDecoder::new(Cursor::new(message_data));
        let mut decompressed = Vec::new();
        match decoder.read_to_end(&mut decompressed) {
            Ok(_) => decompressed,
            Err(e) => {
                common::log_debug!("gRPC decompression failed: {}", e);
                message_data.to_vec()
            }
        }
    } else {
        message_data.to_vec()
    }
}

pub fn decompress_body(body: &[u8], content_encoding: Option<&str>) -> Vec<u8> {
    let Some(encoding) = content_encoding else {
        return body.to_vec();
    };

    let encoding = encoding.to_lowercase();
    if encoding.contains("gzip") {
        let mut decoder = GzDecoder::new(body);
        let mut decompressed = Vec::new();
        match decoder.read_to_end(&mut decompressed) {
            Ok(_) => decompressed,
            Err(e) => {
                common::log_debug!("Failed to decompress gzip body: {}", e);
                body.to_vec()
            }
        }
    } else if encoding.contains("deflate") {
        let mut decoder = DeflateDecoder::new(body);
        let mut decompressed = Vec::new();
        match decoder.read_to_end(&mut decompressed) {
            Ok(_) => decompressed,
            Err(e) => {
                common::log_debug!("Failed to decompress deflate body: {}", e);
                body.to_vec()
            }
        }
    } else if encoding.contains("br") {
        let mut decompressed = Vec::new();
        match brotli::BrotliDecompress(&mut Cursor::new(body), &mut decompressed) {
            Ok(_) => decompressed,
            Err(e) => {
                common::log_debug!("Failed to decompress brotli body: {}", e);
                body.to_vec()
            }
        }
    } else if encoding.contains("zstd") {
        match zstd::decode_all(Cursor::new(body)) {
            Ok(decompressed) => decompressed,
            Err(e) => {
                common::log_debug!("Failed to decompress zstd body: {}", e);
                body.to_vec()
            }
        }
    } else {
        body.to_vec()
    }
}
