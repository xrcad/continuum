//! WASM git backend: calls `window.xrcadGit` (isomorphic-git + ZenFS OPFS)
//! via wasm-bindgen.
//!
//! Compiled only for `target_arch = "wasm32"`.
//!
//! # JS glue required
//!
//! The page bootstrap must load `xrcad-git.js` (see `assets/xrcad-git.js`)
//! **before** the WASM module initialises. That script:
//! 1. Calls `configure({ backend: OPFS })` from `@zenfs/dom` — once, globally.
//! 2. Sets `window.xrcadGit = { init, commit, isInitialised }`.
//!
//! All `JsValue` errors are mapped to `StorageError` at this boundary and
//! never escape into the rest of xrcad.

#![cfg(target_arch = "wasm32")]

use js_sys::{Function, Object, Promise};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use super::{StorageBackend, StorageError};

pub struct IsomorphicGitBackend {
    /// OPFS directory path for this document's git repo, e.g. `/xrcad/my-model`.
    pub dir: String,
}

impl StorageBackend for IsomorphicGitBackend {
    async fn init(&self) -> Result<(), StorageError> {
        let xrcad_git = get_xrcad_git()?;
        let f = get_fn(&xrcad_git, "init")?;
        let promise: Promise = f
            .call1(&JsValue::NULL, &JsValue::from_str(&self.dir))
            .map_err(|e| StorageError::Git(format!("xrcadGit.init call failed: {e:?}")))?
            .into();
        JsFuture::from(promise)
            .await
            .map_err(|e| StorageError::Git(format!("xrcadGit.init rejected: {e:?}")))?;
        Ok(())
    }

    async fn commit(&self, message: &str, ops_content: &str) -> Result<(), StorageError> {
        let xrcad_git = get_xrcad_git()?;
        let f = get_fn(&xrcad_git, "commit")?;
        let promise: Promise = f
            .call3(
                &JsValue::NULL,
                &JsValue::from_str(&self.dir),
                &JsValue::from_str(message),
                &JsValue::from_str(ops_content),
            )
            .map_err(|e| StorageError::Git(format!("xrcadGit.commit call failed: {e:?}")))?
            .into();
        JsFuture::from(promise)
            .await
            .map_err(|e| StorageError::Git(format!("xrcadGit.commit rejected: {e:?}")))?;
        Ok(())
    }

    async fn is_initialised(&self) -> bool {
        let Ok(xrcad_git) = get_xrcad_git() else { return false };
        let Ok(f) = get_fn(&xrcad_git, "isInitialised") else { return false };
        let Ok(val) = f.call1(&JsValue::NULL, &JsValue::from_str(&self.dir)) else {
            return false;
        };
        let promise: Promise = val.into();
        match JsFuture::from(promise).await {
            Ok(v) => v.as_bool().unwrap_or(false),
            Err(_) => false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn get_xrcad_git() -> Result<Object, StorageError> {
    let window = web_sys::window()
        .ok_or_else(|| StorageError::Git("no window object".into()))?;
    js_sys::Reflect::get(&window, &JsValue::from_str("xrcadGit"))
        .map_err(|_| StorageError::Git("window.xrcadGit not found".into()))?
        .dyn_into::<Object>()
        .map_err(|_| StorageError::Git("window.xrcadGit is not an object".into()))
}

fn get_fn(obj: &Object, name: &str) -> Result<Function, StorageError> {
    js_sys::Reflect::get(obj, &JsValue::from_str(name))
        .map_err(|_| StorageError::Git(format!("window.xrcadGit.{name} not found")))?
        .dyn_into::<Function>()
        .map_err(|_| StorageError::Git(format!("window.xrcadGit.{name} is not a function")))
}
