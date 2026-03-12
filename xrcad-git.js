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

import LightningFS from '@isomorphic-git/lightning-fs';
import git from 'isomorphic-git';

const _fs = new LightningFS('xrcad');
const fs = { promises: _fs.promises };

window.xrcadGit = {
    /**
     * Initialise a git repository at `dir` if one does not exist.
     * @param {string} dir  OPFS path, e.g. "/xrcad/my-model"
     */
    async init(dir) {
        if (!fs) throw new Error('[xrcad-git] filesystem not available; OPFS initialisation failed');
        try {
            await git.init({ fs, dir });
        } catch (err) {
            console.error('[xrcad-git] init failed:', err);
            throw err;
        }
    },

    /**
     * Write `opsContent` to ops.log, stage it, and create a commit.
     * @param {string} dir
     * @param {string} message   Full commit message (structured format)
     * @param {string} opsContent  Complete text content of ops.log
     */
    async commit(dir, message, opsContent) {
        if (!fs) throw new Error('[xrcad-git] filesystem not available; OPFS initialisation failed');
        try {
            await fs.promises.writeFile(`${dir}/ops.log`, opsContent, 'utf8');
            await git.add({ fs, dir, filepath: 'ops.log' });
            await git.commit({
                fs,
                dir,
                message,
                author: { name: 'xrcad', email: 'xrcad@local' },
            });
        } catch (err) {
            console.error('[xrcad-git] commit failed:', err);
            throw err;
        }
    },

    /**
     * Returns true if the repo at `dir` has at least one commit (HEAD resolves).
     * @param {string} dir
     * @returns {Promise<boolean>}
     */
    async isInitialised(dir) {
        if (!fs) return false;
        try {
            await git.resolveRef({ fs, dir, ref: 'HEAD' });
            return true;
        } catch {
            return false;
        }
    },
};
