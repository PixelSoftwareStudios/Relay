import { fileURLToPath } from "node:url";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

// Shared fonts and images live in the repository-level assets folder.
const assets = fileURLToPath(new URL("../../assets", import.meta.url));

export default defineConfig({
	plugins: [svelte()],
	resolve: { alias: { $assets: assets } },
	server: { port: 1420, strictPort: true, fs: { allow: [".", assets] } },
	clearScreen: false,
	build: { target: "es2022", outDir: "dist", emptyOutDir: true },
	test: {
		environment: "jsdom",
		include: ["tests/**/*.test.ts"],
	},
});
