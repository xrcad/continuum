/**
 * xrcad-git.js — isomorphic-git + ZenFS OPFS glue.
 *
 * Must be imported BEFORE the WASM module loads. Sets window.xrcadGit.
 *
 * Requirements:
 *   npm install isomorphic-git @zenfs/core @zenfs/dom
 *
 * COOP/COEP headers are required for OPFS. GitHub Pages does not support
 * custom headers — use the coi-serviceworker.js approach (see sw.js).
 */

import { configure } from '@zenfs/core';
import { OPFS } from '@zenfs/dom';
import git from 'isomorphic-git';

// Configure ZenFS once at startup to use OPFS as the filesystem backend.
// OPFS is persistent, sandboxed to the origin, and requires no user gesture.
await configure({ backend: OPFS });

// Import the ZenFS fs module AFTER configure() resolves.
const { fs } = await import('@zenfs/core');

window.xrcadGit = {
    /**
     * Initialise a git repository at `dir` if one does not exist.
     * @param {string} dir  OPFS path, e.g. "/xrcad/my-model"
     */
    async init(dir) {
        await git.init({ fs, dir });
    },

    /**
     * Write `opsContent` to ops.log, stage it, and create a commit.
     * @param {string} dir
     * @param {string} message   Full commit message (structured format)
     * @param {string} opsContent  Complete text content of ops.log
     */
    async commit(dir, message, opsContent) {
        await fs.promises.writeFile(`${dir}/ops.log`, opsContent, 'utf8');
        await git.add({ fs, dir, filepath: 'ops.log' });
        await git.commit({
            fs,
            dir,
            message,
            author: { name: 'xrcad', email: 'xrcad@local' },
        });
    },

    /**
     * Returns true if the repo at `dir` has at least one commit (HEAD resolves).
     * @param {string} dir
     * @returns {Promise<boolean>}
     */
    async isInitialised(dir) {
        try {
            await git.resolveRef({ fs, dir, ref: 'HEAD' });
            return true;
        } catch {
            return false;
        }
    },
};
