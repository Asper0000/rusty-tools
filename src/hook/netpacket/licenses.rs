//! `Hooks_NetPacket_Licenses`. Incoming `CMsgClientLicenseList` (eMsg
//! 780). Read-only: the message is handed on untouched. Parses the
//! license list and hands it to `Hooks_Package::OnLicenseList`.

use prost::Message;

use crate::hook::package::{self, License};
use crate::steam::proto::CMsgClientLicenseList;

pub fn handle_recv(body: &[u8]) {
    let msg = match CMsgClientLicenseList::decode(body) {
        Ok(m) => m,
        Err(_) => {
            LOG_PACKAGE_WARN!("LicenseList: failed to parse CMsgClientLicenseList");
            return;
        }
    };

    let mut licenses = Vec::with_capacity(msg.licenses.len());
    for lic in &msg.licenses {
        let Some(package_id) = lic.package_id else { continue };
        licenses.push(License {
            package_id,
            access_token: lic.access_token.unwrap_or(0),
        });
    }

    LOG_PACKAGE_INFO!(
        "LicenseList: {} license(s), eresult={}",
        licenses.len(), msg.eresult.unwrap_or(0),
    );

    package::on_license_list(licenses);
    package::try_dump_owned_depots();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steam::proto::CMsgClientLicenseList;
    use crate::steam::proto::c_msg_client_license_list::License as ProtoLicense;

    fn encode(msg: &CMsgClientLicenseList) -> Vec<u8> {
        msg.encode_to_vec()
    }

    #[test]
    fn malformed_body_does_not_panic() {
        handle_recv(&[0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn parse_empty_list_is_ok() {
        let msg = CMsgClientLicenseList {
            eresult: Some(1),
            licenses: vec![],
        };
        handle_recv(&encode(&msg));
    }

    #[test]
    fn parse_one_license() {
        let msg = CMsgClientLicenseList {
            eresult: Some(1),
            licenses: vec![ProtoLicense {
                package_id: Some(0x1234),
                access_token: Some(0xCAFE_BABE),
                ..Default::default()
            }],
        };
        handle_recv(&encode(&msg));
    }
}
