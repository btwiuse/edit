// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

/**
 * Service Worker for the Edit browser PWA.
 *
 * Caches core app assets on installation so the editor can load offline
 * after the first visit.  CDN-hosted xterm.js assets are also cached on
 * first fetch so that subsequent loads work without a network connection.
 */

const CACHE_NAME = "edit-v1";

// Core assets bundled with the app.
const PRECACHE_URLS = [
  "./",
  "./main.js",
  "./manifest.json",
  "./pkg/edit_wasm.js",
  "./pkg/edit_wasm_bg.wasm",
];

// ── install ───────────────────────────────────────────────────────────────────

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE_NAME)
      .then((cache) => cache.addAll(PRECACHE_URLS))
      .then(() => self.skipWaiting())
  );
});

// ── activate ──────────────────────────────────────────────────────────────────

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => key !== CACHE_NAME)
            .map((key) => caches.delete(key))
        )
      )
      .then(() => self.clients.claim())
  );
});

// ── fetch ─────────────────────────────────────────────────────────────────────

self.addEventListener("fetch", (event) => {
  // Only handle GET requests.
  if (event.request.method !== "GET") return;

  event.respondWith(
    caches.match(event.request).then((cached) => {
      if (cached) {
        return cached;
      }

      // Not in cache – fetch from network and cache the response.
      return fetch(event.request)
        .then((response) => {
          // Only cache successful, non-opaque responses.
          if (
            response.ok ||
            (response.type === "opaque" && response.status === 0)
          ) {
            const clone = response.clone();
            caches.open(CACHE_NAME).then((cache) => cache.put(event.request, clone));
          }
          return response;
        })
        .catch(() => cached); // Return cached copy if network fails.
    })
  );
});
