// Copyright 2023 Adobe. All rights reserved.
// This file is licensed to you under the Apache License,
// Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0)
// or the MIT license (http://opensource.org/licenses/MIT),
// at your option.

// Unless required by applicable law or agreed to in writing,
// this software is distributed on an "AS IS" BASIS, WITHOUT
// WARRANTIES OR REPRESENTATIONS OF ANY KIND, either express or
// implied. See the LICENSE-MIT and LICENSE-APACHE files for the
// specific language governing permissions and limitations under
// each license.

use std::{fs::File, path::Path};

use crate::{
    asset_handlers::pdf::{C2paPdf, Pdf},
    asset_io::{
        AssetIO, CAIRead, CAIReadWrite, CAIReader, CAIWriter, ComposedManifestRef,
        HashObjectPositions,
    },
    utils::patch::patch_bytes,
    Error::{self, JumbfNotFound, NotImplemented, PdfReadError},
};

static SUPPORTED_TYPES: [&str; 2] = ["pdf", "application/pdf"];
static WRITE_NOT_IMPLEMENTED: &str = "PDF write functionality will be added in a future release";

pub struct PdfIO {}

impl CAIReader for PdfIO {
    fn read_cai(&self, asset_reader: &mut dyn CAIRead) -> crate::Result<Vec<u8>> {
        asset_reader.rewind()?;
        let pdf = Pdf::from_reader(asset_reader).map_err(|e| Error::InvalidAsset(e.to_string()))?;
        self.read_manifest_bytes(pdf)
    }

    fn read_xmp(&self, asset_reader: &mut dyn CAIRead) -> Option<String> {
        if asset_reader.rewind().is_err() {
            return None;
        }

        let Ok(pdf) = Pdf::from_reader(asset_reader) else {
            return None;
        };

        self.read_xmp_from_pdf(pdf)
    }
}

impl CAIWriter for PdfIO {
    fn write_cai(
        &self,
        input_stream: &mut dyn CAIRead,
        output_stream: &mut dyn CAIReadWrite,
        store_bytes: &[u8],
    ) -> crate::Result<()> {
        input_stream.rewind()?;
        let mut pdf_bytes = Vec::new();
        input_stream.read_to_end(&mut pdf_bytes)?;

        let mut pdf =
            Pdf::from_bytes(&pdf_bytes).map_err(|e| Error::InvalidAsset(e.to_string()))?;

        if let Some(manifests) = pdf
            .read_manifest_bytes()
            .map_err(|e| Error::InvalidAsset(e.to_string()))?
        {
            let (current_manifest, _) = manifests.first().ok_or(Error::JumbfNotFound)?;
            patch_bytes(&mut pdf_bytes, current_manifest, store_bytes)?;
            output_stream.rewind()?;
            output_stream.write_all(&pdf_bytes)?;
        } else {
            pdf.write_manifest_as_embedded_file(store_bytes.to_vec())
                .map_err(|e| Error::InvalidAsset(e.to_string()))?;

            let mut out_buf = Vec::new();
            pdf.save_to(&mut out_buf)?;

            output_stream.rewind()?;
            output_stream.write_all(&out_buf)?;
        }

        Ok(())
    }

    fn get_object_locations_from_stream(
        &self,
        input_stream: &mut dyn CAIRead,
    ) -> crate::Result<Vec<HashObjectPositions>> {
        input_stream.rewind()?;
        let mut pdf =
            Pdf::from_reader(input_stream).map_err(|e| Error::InvalidAsset(e.to_string()))?;

        if let Some(manifests) = pdf
            .read_manifest_bytes()
            .map_err(|e| Error::InvalidAsset(e.to_string()))?
        {
            let (current_manifest, offset) = manifests.first().ok_or(Error::JumbfNotFound)?;

            Ok(vec![HashObjectPositions {
                offset: *offset,
                length: current_manifest.len(),
                htype: crate::asset_io::HashBlockObjectType::Cai,
            }])
        } else {
            // Write a single byte as a placeholder manifest.
            pdf.write_manifest_as_embedded_file(vec![0])
                .map_err(|e| Error::InvalidAsset(e.to_string()))?;

            let mut out = Vec::new();
            pdf.save_to(&mut out)?;

            let pdf = Pdf::from_bytes(&out).map_err(|e| Error::InvalidAsset(e.to_string()))?;

            let manifests = pdf
                .read_manifest_bytes()
                .map_err(|e| Error::InvalidAsset(e.to_string()))?
                .ok_or(Error::JumbfNotFound)?;

            let (current_manifest, offset) = manifests.first().ok_or(Error::JumbfNotFound)?;

            Ok(vec![HashObjectPositions {
                offset: *offset,
                length: current_manifest.len(),
                htype: crate::asset_io::HashBlockObjectType::Cai,
            }])
        }
    }

    fn remove_cai_store_from_stream(
        &self,
        mut input_stream: &mut dyn CAIRead,
        output_stream: &mut dyn CAIReadWrite,
    ) -> crate::Result<()> {
        input_stream.rewind()?;
        let mut pdf =
            Pdf::from_reader(&mut input_stream).map_err(|e| Error::InvalidAsset(e.to_string()))?;

        if pdf
            .read_manifest_bytes()
            .map_err(|e| Error::InvalidAsset(e.to_string()))?
            .is_some()
        {
            pdf.remove_manifest_bytes()
                .map_err(|e| Error::InvalidAsset(e.to_string()))?;

            let mut out_buf = Vec::new();
            pdf.save_to(&mut out_buf)?;

            output_stream.rewind()?;
            output_stream.write_all(&out_buf)?;
        } else {
            input_stream.rewind()?;
            std::io::copy(input_stream, output_stream)?;
        }

        Ok(())
    }
}

impl PdfIO {
    fn read_manifest_bytes(&self, pdf: impl C2paPdf) -> crate::Result<Vec<u8>> {
        let Ok(result) = pdf.read_manifest_bytes() else {
            return Err(PdfReadError);
        };

        let Some(bytes) = result else {
            return Err(JumbfNotFound);
        };

        match bytes.as_slice() {
            [(bytes, _)] => Ok(bytes.to_vec()),
            _ => Err(NotImplemented(
                "c2pa-rs only supports reading PDFs with one manifest".into(),
            )),
        }
    }

    fn read_xmp_from_pdf(&self, pdf: impl C2paPdf) -> Option<String> {
        pdf.read_xmp()
    }
}

impl AssetIO for PdfIO {
    fn new(_asset_type: &str) -> Self
    where
        Self: Sized,
    {
        Self {}
    }

    fn get_handler(&self, asset_type: &str) -> Box<dyn AssetIO> {
        Box::new(PdfIO::new(asset_type))
    }

    fn get_reader(&self) -> &dyn CAIReader {
        self
    }

    fn get_writer(&self, _asset_type: &str) -> Option<Box<dyn CAIWriter>> {
        Some(Box::new(PdfIO {}))
    }

    fn read_cai_store(&self, asset_path: &Path) -> crate::Result<Vec<u8>> {
        let mut f = File::open(asset_path)?;
        self.read_cai(&mut f)
    }

    fn save_cai_store(&self, _asset_path: &Path, _store_bytes: &[u8]) -> crate::Result<()> {
        Err(NotImplemented(WRITE_NOT_IMPLEMENTED.into()))
    }

    fn get_object_locations(&self, _asset_path: &Path) -> crate::Result<Vec<HashObjectPositions>> {
        Err(NotImplemented(WRITE_NOT_IMPLEMENTED.into()))
    }

    fn remove_cai_store(&self, _asset_path: &Path) -> crate::Result<()> {
        Err(NotImplemented(WRITE_NOT_IMPLEMENTED.into()))
    }

    fn supported_types(&self) -> &[&str] {
        &SUPPORTED_TYPES
    }

    fn composed_data_ref(&self) -> Option<&dyn ComposedManifestRef> {
        Some(self)
    }
}

impl ComposedManifestRef for PdfIO {
    // Return entire CAI block as Vec<u8>
    fn compose_manifest(&self, manifest_data: &[u8], _format: &str) -> Result<Vec<u8>, Error> {
        Ok(manifest_data.to_vec())
    }
}

#[cfg(test)]
pub mod tests {
    #![allow(clippy::panic)]
    #![allow(clippy::unwrap_used)]

    use std::io::Cursor;

    use crate::{
        asset_handlers,
        asset_handlers::{pdf::MockC2paPdf, pdf_io::PdfIO},
        asset_io::{AssetIO, CAIReader},
    };

    static MANIFEST_BYTES: &[u8; 2] = &[10u8, 20u8];

    #[test]
    fn test_error_reading_manifest_fails() {
        let mut mock_pdf = MockC2paPdf::default();
        mock_pdf.expect_read_manifest_bytes().returning(|| {
            Err(asset_handlers::pdf::Error::UnableToReadPdf(
                lopdf::Error::ReferenceLimit,
            ))
        });

        let pdf_io = PdfIO::new("pdf");
        assert!(matches!(
            pdf_io.read_manifest_bytes(mock_pdf),
            Err(crate::Error::PdfReadError)
        ))
    }

    #[test]
    fn test_no_manifest_found_returns_no_jumbf_error() {
        let mut mock_pdf = MockC2paPdf::default();
        mock_pdf.expect_read_manifest_bytes().returning(|| Ok(None));
        let pdf_io = PdfIO::new("pdf");

        assert!(matches!(
            pdf_io.read_manifest_bytes(mock_pdf),
            Err(crate::Error::JumbfNotFound)
        ));
    }

    #[test]
    fn test_one_manifest_found_returns_bytes() {
        let mut mock_pdf = MockC2paPdf::default();
        mock_pdf
            .expect_read_manifest_bytes()
            .returning(|| Ok(Some(vec![MANIFEST_BYTES])));

        let pdf_io = PdfIO::new("pdf");
        assert_eq!(
            pdf_io.read_manifest_bytes(mock_pdf).unwrap(),
            MANIFEST_BYTES.to_vec()
        );
    }

    #[test]
    fn test_multiple_manifest_fail_with_not_implemented_error() {
        let mut mock_pdf = MockC2paPdf::default();
        mock_pdf
            .expect_read_manifest_bytes()
            .returning(|| Ok(Some(vec![MANIFEST_BYTES, MANIFEST_BYTES, MANIFEST_BYTES])));

        let pdf_io = PdfIO::new("pdf");

        assert!(matches!(
            pdf_io.read_manifest_bytes(mock_pdf),
            Err(crate::Error::NotImplemented(_))
        ));
    }

    #[test]
    fn test_returns_none_when_no_xmp() {
        let mut mock_pdf = MockC2paPdf::default();
        mock_pdf.expect_read_xmp().returning(|| None);

        let pdf_io = PdfIO::new("pdf");
        assert!(pdf_io.read_xmp_from_pdf(mock_pdf).is_none());
    }

    #[test]
    fn test_returns_some_when_some_xmp() {
        let mut mock_pdf = MockC2paPdf::default();
        mock_pdf.expect_read_xmp().returning(|| Some("xmp".into()));

        let pdf_io = PdfIO::new("pdf");
        assert!(pdf_io.read_xmp_from_pdf(mock_pdf).is_some());
    }

    #[test]
    fn test_cai_read_finds_no_manifest() {
        let source = crate::utils::test::fixture_path("basic.pdf");
        let pdf_io = PdfIO::new("pdf");

        assert!(matches!(
            pdf_io.read_cai_store(&source),
            Err(crate::Error::JumbfNotFound)
        ));
    }

    #[test]
    fn test_cai_read_xmp_finds_xmp_data() {
        let source = include_bytes!("../../tests/fixtures/basic.pdf");
        let mut stream = Cursor::new(source.to_vec());

        let pdf_io = PdfIO::new("pdf");
        assert!(pdf_io.read_xmp(&mut stream).is_some());
    }

    #[test]
    fn test_read_cai_express_pdf_finds_single_manifest_store() {
        let source = include_bytes!("../../tests/fixtures/express-signed.pdf");
        let pdf_io = PdfIO::new("pdf");
        let mut pdf_stream = Cursor::new(source.to_vec());
        assert!(pdf_io.read_cai(&mut pdf_stream).is_ok());
    }
}
