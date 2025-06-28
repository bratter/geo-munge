use crate::message::prelude::*;

use super::Context;

pub fn delete(context: Context, key_set: KeySet) {
    let store = context.store.load();
    let mut success: usize = 0;
    let mut fail: usize = 0;

    match key_set {
        KeySet::Uid(keys) => {
            for key in keys {
                if store.delete(&key) {
                    success += 1;
                } else {
                    fail += 1;
                }
            }
        }
        KeySet::Custom(keys) => {
            for key in keys {
                if store.delete_with_custom_key(&key) {
                    success += 1;
                } else {
                    fail += 1;
                }
            }
        }
    }

    context.send(Response::ResultCounts { success, fail })
}
