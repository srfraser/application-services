/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::error::*;
use crate::util;
use rusqlite::Row;
use serde_derive::*;
use std::time::{self, SystemTime};
use sync15::ServerTimestamp;
use sync_guid::Guid;

#[derive(Debug, Clone, Hash, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    #[serde(rename = "id")]
    pub guid: Guid,

    pub hostname: String,

    // rename_all = "camelCase" by default will do formSubmitUrl, but we can just
    // override this one field.
    #[serde(rename = "formSubmitURL")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form_submit_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_realm: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub username: String,

    pub password: String,

    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub username_field: String,

    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub password_field: String,

    #[serde(default)]
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub time_created: i64,

    #[serde(default)]
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub time_password_changed: i64,

    #[serde(default)]
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub time_last_used: i64,

    #[serde(default)]
    pub times_used: i64,
}

fn deserialize_timestamp<'de, D>(deserializer: D) -> std::result::Result<i64, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    use serde::de::Deserialize;
    // Invalid and negative timestamps are all replaced with 0. Eventually we
    // should investigate replacing values that are unreasonable but still fit
    // in an i64 (a date 1000 years in the future, for example), but
    // appropriately handling that is complex.
    Ok(i64::deserialize(deserializer).unwrap_or_default().max(0))
}

fn string_or_default(row: &Row<'_>, col: &str) -> Result<String> {
    Ok(row.get::<_, Option<String>>(col)?.unwrap_or_default())
}

impl Address {
    #[inline]
    pub fn guid(&self) -> &Guid {
        &self.guid
    }

    #[inline]
    pub fn guid_str(&self) -> &str {
        self.guid.as_str()
    }

    pub fn check_valid(&self) -> Result<()> {
        if self.hostname.is_empty() {
            throw!(InvalidAddress::EmptyHostname);
        }

        if self.password.is_empty() {
            throw!(InvalidAddress::EmptyPassword);
        }

        if self.form_submit_url.is_some() && self.http_realm.is_some() {
            throw!(InvalidAddress::BothTargets);
        }

        if self.form_submit_url.is_none() && self.http_realm.is_none() {
            throw!(InvalidAddress::NoTarget);
        }
        Ok(())
    }

    pub(crate) fn from_row(row: &Row<'_>) -> Result<Address> {
        Ok(Address {
            guid: row.get("guid")?,
            password: row.get("password")?,
            username: string_or_default(row, "username")?,

            hostname: row.get("hostname")?,
            http_realm: row.get("httpRealm")?,

            form_submit_url: row.get("formSubmitURL")?,

            username_field: string_or_default(row, "usernameField")?,
            password_field: string_or_default(row, "passwordField")?,

            time_created: row.get("timeCreated")?,
            // Might be null
            time_last_used: row
                .get::<_, Option<i64>>("timeLastUsed")?
                .unwrap_or_default(),

            time_password_changed: row.get("timePasswordChanged")?,
            times_used: row.get("timesUsed")?,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MirrorAddress {
    pub address: Address,
    pub is_overridden: bool,
    pub server_modified: ServerTimestamp,
}

impl MirrorAddress {
    #[inline]
    pub fn guid_str(&self) -> &str {
        self.address.guid_str()
    }

    pub(crate) fn from_row(row: &Row<'_>) -> Result<MirrorAddress> {
        Ok(MirrorAddress {
            address: Address::from_row(row)?,
            is_overridden: row.get("is_overridden")?,
            server_modified: ServerTimestamp(row.get::<_, i64>("server_modified")?),
        })
    }
}

// This doesn't really belong here.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(u8)]
pub(crate) enum SyncStatus {
    Synced = 0,
    Changed = 1,
    New = 2,
}

impl SyncStatus {
    #[inline]
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(SyncStatus::Synced),
            1 => Ok(SyncStatus::Changed),
            2 => Ok(SyncStatus::New),
            v => throw!(ErrorKind::BadSyncStatus(v)),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LocalAddress {
    pub address: Address,
    pub sync_status: SyncStatus,
    pub is_deleted: bool,
    pub local_modified: SystemTime,
}

impl LocalAddress {
    #[inline]
    pub fn guid_str(&self) -> &str {
        self.address.guid_str()
    }

    pub(crate) fn from_row(row: &Row<'_>) -> Result<LocalAddress> {
        Ok(LocalAddress {
            address: Address::from_row(row)?,
            sync_status: SyncStatus::from_u8(row.get("sync_status")?)?,
            is_deleted: row.get("is_deleted")?,
            local_modified: util::system_time_millis_from_row(row, "local_modified")?,
        })
    }
}

macro_rules! impl_address {
    ($ty:ty { $($fields:tt)* }) => {
        impl AsRef<Address> for $ty {
            #[inline]
            fn as_ref(&self) -> &Address {
                &self.address
            }
        }

        impl AsMut<Address> for $ty {
            #[inline]
            fn as_mut(&mut self) -> &mut Address {
                &mut self.address
            }
        }

        impl From<$ty> for Address {
            #[inline]
            fn from(l: $ty) -> Self {
                l.address
            }
        }

        impl From<Address> for $ty {
            #[inline]
            fn from(address: Address) -> Self {
                Self { address, $($fields)* }
            }
        }
    };
}

impl_address!(LocalAddress {
    sync_status: SyncStatus::New,
    is_deleted: false,
    local_modified: time::UNIX_EPOCH
});

impl_address!(MirrorAddress {
    is_overridden: false,
    server_modified: ServerTimestamp(0)
});

// Stores data needed to do a 3-way merge
pub(crate) struct SyncAddressData {
    pub guid: Guid,
    pub local: Option<LocalAddress>,
    pub mirror: Option<MirrorAddress>,
    // None means it's a deletion
    pub inbound: (Option<Address>, ServerTimestamp),
}

impl SyncAddressData {
    #[inline]
    pub fn guid_str(&self) -> &str {
        &self.guid.as_str()
    }

    #[inline]
    pub fn guid(&self) -> &Guid {
        &self.guid
    }

    // Note: fetch_address_data in db.rs assumes that this can only fail with a deserialization error. Currently, this is true,
    // but you'll need to adjust that function if you make this return another type of Result.
    pub fn from_payload(
        payload: sync15::Payload,
        ts: ServerTimestamp,
    ) -> std::result::Result<Self, serde_json::Error> {
        let guid = payload.id.clone();
        let address: Option<Address> = if payload.is_tombstone() {
            None
        } else {
            let record: Address = payload.into_record()?;
            Some(record)
        };
        Ok(Self {
            guid,
            local: None,
            mirror: None,
            inbound: (address, ts),
        })
    }
}

macro_rules! impl_address_setter {
    ($setter_name:ident, $field:ident, $Address:ty) => {
        impl SyncAddressData {
            pub(crate) fn $setter_name(&mut self, record: $Address) -> Result<()> {
                // TODO: We probably shouldn't panic in this function!
                if self.$field.is_some() {
                    // Shouldn't be possible (only could happen if UNIQUE fails in sqlite, or if we
                    // get duplicate guids somewhere,but we check).
                    panic!(
                        "SyncAddressData::{} called on object that already has {} data",
                        stringify!($setter_name),
                        stringify!($field)
                    );
                }

                if self.guid_str() != record.guid_str() {
                    // This is almost certainly a bug in our code.
                    panic!(
                        "Wrong guid on address in {}: {:?} != {:?}",
                        stringify!($setter_name),
                        self.guid_str(),
                        record.guid_str()
                    );
                }

                self.$field = Some(record);
                Ok(())
            }
        }
    };
}

impl_address_setter!(set_local, local, LocalAddress);
impl_address_setter!(set_mirror, mirror, MirrorAddress);

#[derive(Debug, Default, Clone)]
pub(crate) struct AddressDelta {
    // "non-commutative" fields
    pub hostname: Option<String>,
    pub password: Option<String>,
    pub username: Option<String>,
    pub http_realm: Option<String>,
    pub form_submit_url: Option<String>,

    pub time_created: Option<i64>,
    pub time_last_used: Option<i64>,
    pub time_password_changed: Option<i64>,

    // "non-conflicting" fields (which are the same)
    pub password_field: Option<String>,
    pub username_field: Option<String>,

    // Commutative field
    pub times_used: i64,
}

macro_rules! merge_field {
    ($merged:ident, $b:ident, $prefer_b:expr, $field:ident) => {
        if let Some($field) = $b.$field.take() {
            if $merged.$field.is_some() {
                log::warn!("Collision merging address field {}", stringify!($field));
                if $prefer_b {
                    $merged.$field = Some($field);
                }
            } else {
                $merged.$field = Some($field);
            }
        }
    };
}

impl AddressDelta {
    #[allow(clippy::cognitive_complexity)] // Looks like clippy considers this after macro-expansion...
    pub fn merge(self, mut b: AddressDelta, b_is_newer: bool) -> AddressDelta {
        let mut merged = self;
        merge_field!(merged, b, b_is_newer, hostname);
        merge_field!(merged, b, b_is_newer, password);
        merge_field!(merged, b, b_is_newer, username);
        merge_field!(merged, b, b_is_newer, http_realm);
        merge_field!(merged, b, b_is_newer, form_submit_url);

        merge_field!(merged, b, b_is_newer, time_created);
        merge_field!(merged, b, b_is_newer, time_last_used);
        merge_field!(merged, b, b_is_newer, time_password_changed);

        merge_field!(merged, b, b_is_newer, password_field);
        merge_field!(merged, b, b_is_newer, username_field);

        // commutative fields
        merged.times_used += b.times_used;

        merged
    }
}

macro_rules! apply_field {
    ($address:ident, $delta:ident, $field:ident) => {
        if let Some($field) = $delta.$field.take() {
            $address.$field = $field.into();
        }
    };
}

impl Address {
    pub(crate) fn apply_delta(&mut self, mut delta: AddressDelta) {
        apply_field!(self, delta, hostname);

        apply_field!(self, delta, password);
        apply_field!(self, delta, username);

        apply_field!(self, delta, time_created);
        apply_field!(self, delta, time_last_used);
        apply_field!(self, delta, time_password_changed);

        apply_field!(self, delta, password_field);
        apply_field!(self, delta, username_field);

        // Use Some("") to indicate that it should be changed to be None (hacky...)
        if let Some(realm) = delta.http_realm.take() {
            self.http_realm = if realm.is_empty() { None } else { Some(realm) };
        }

        if let Some(url) = delta.form_submit_url.take() {
            self.form_submit_url = if url.is_empty() { None } else { Some(url) };
        }

        self.times_used += delta.times_used;
    }

    pub(crate) fn delta(&self, older: &Address) -> AddressDelta {
        let mut delta = AddressDelta::default();

        if self.form_submit_url != older.form_submit_url {
            delta.form_submit_url = Some(self.form_submit_url.clone().unwrap_or_default());
        }

        if self.http_realm != older.http_realm {
            delta.http_realm = Some(self.http_realm.clone().unwrap_or_default());
        }

        if self.hostname != older.hostname {
            delta.hostname = Some(self.hostname.clone());
        }
        if self.username != older.username {
            delta.username = Some(self.username.clone());
        }
        if self.password != older.password {
            delta.password = Some(self.password.clone());
        }
        if self.password_field != older.password_field {
            delta.password_field = Some(self.password_field.clone());
        }
        if self.username_field != older.username_field {
            delta.username_field = Some(self.username_field.clone());
        }

        // We discard zero (and negative numbers) for timestamps so that a
        // record that doesn't contain this information (these are
        // `#[serde(default)]`) doesn't skew our records.
        //
        // Arguably, we should also also ignore values later than our
        // `time_created`, or earlier than our `time_last_used` or
        // `time_password_changed`. Doing this properly would probably require
        // a scheme analogous to Desktop's weak-reupload system, so I'm punting
        // on it for now.
        if self.time_created > 0 && self.time_created != older.time_created {
            delta.time_created = Some(self.time_created);
        }
        if self.time_last_used > 0 && self.time_last_used != older.time_last_used {
            delta.time_last_used = Some(self.time_last_used);
        }
        if self.time_password_changed > 0
            && self.time_password_changed != older.time_password_changed
        {
            delta.time_password_changed = Some(self.time_password_changed);
        }

        if self.times_used > 0 && self.times_used != older.times_used {
            delta.times_used = self.times_used - older.times_used;
        }

        delta
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_invalid_payload_timestamp() {
        #[allow(clippy::unreadable_literal)]
        let bad_timestamp = 18446732429235952000u64;
        let bad_payload: sync15::Payload = serde_json::from_value(serde_json::json!({
            "id": "123412341234",
            "formSubmitURL": "https://www.example.com/submit",
            "hostname": "https://www.example.com",
            "username": "test",
            "password": "test",
            "timeCreated": bad_timestamp,
            "timeLastUsed": "some other garbage",
            "timePasswordChanged": -30, // valid i64 but negative
        }))
        .unwrap();
        let address = SyncAddressData::from_payload(bad_payload, ServerTimestamp::default())
            .unwrap()
            .inbound
            .0
            .unwrap();
        assert_eq!(address.time_created, 0);
        assert_eq!(address.time_last_used, 0);
        assert_eq!(address.time_password_changed, 0);

        let now64 = util::system_time_ms_i64(std::time::SystemTime::now());
        let good_payload: sync15::Payload = serde_json::from_value(serde_json::json!({
            "id": "123412341234",
            "formSubmitURL": "https://www.example.com/submit",
            "hostname": "https://www.example.com",
            "username": "test",
            "password": "test",
            "timeCreated": now64 - 100,
            "timeLastUsed": now64 - 50,
            "timePasswordChanged": now64 - 25,
        }))
        .unwrap();

        let address = SyncAddressData::from_payload(good_payload, ServerTimestamp::default())
            .unwrap()
            .inbound
            .0
            .unwrap();

        assert_eq!(address.time_created, now64 - 100);
        assert_eq!(address.time_last_used, now64 - 50);
        assert_eq!(address.time_password_changed, now64 - 25);
    }
}