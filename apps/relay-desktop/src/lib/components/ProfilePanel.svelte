<script lang="ts">
	import { avatarProblem } from "../format";
	import type { Bridge } from "../types";
	import Avatar from "./Avatar.svelte";

	let { busy, api, run }: { busy: boolean; api: Bridge; run: (action: () => Promise<void>) => Promise<void> } = $props();

	let name = $state("");
	let preview = $state<string | null>(null);
	let problem = $state<string | null>(null);

	async function pick(e: Event) {
		const input = e.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		input.value = "";
		if (!file) return;
		problem = avatarProblem(file.size, file.type);
		if (problem) return;
		const bytes = new Uint8Array(await file.arrayBuffer());
		await run(() => api.uploadAvatar(bytes, file.type));
		if (preview) URL.revokeObjectURL(preview);
		preview = URL.createObjectURL(file);
	}
</script>

<section class="stack" aria-labelledby="prof-h">
	<h2 id="prof-h">Your profile</h2>
	<div class="card stack">
		<div class="row">
			<Avatar name={name || "Me"} id="self" src={preview} size={48} />
			<label class="grow stack">
				<span>Profile image (PNG or JPEG, up to 256 KiB)</span>
				<input type="file" accept="image/png,image/jpeg" disabled={busy} onchange={pick} />
			</label>
		</div>
		{#if problem}<div class="banner error" role="alert">{problem}</div>{/if}
		<form class="row" onsubmit={(e) => { e.preventDefault(); void run(() => api.setProfile(name.trim())); }}>
			<input class="grow" type="text" maxlength="64" placeholder="Display name" bind:value={name} aria-label="Display name" />
			<button type="submit" disabled={busy || name.trim() === ""}>Save name</button>
		</form>
		<p class="muted">The image is previewed here only; the engine shares it with peers.</p>
	</div>
</section>
