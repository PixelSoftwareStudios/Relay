<script lang="ts">
	import { linkLabel } from "../format";
	import type { AppState } from "../types";

	let { state, error }: { state: AppState; error: string | null } = $props();
</script>

<div class="stack" aria-live="polite">
	{#if state.mock}
		<div class="banner">Mock data: this view is not connected to a Relay daemon.</div>
	{:else if state.link !== "connected"}
		<div class="banner" class:error={state.link === "offline"}>
			{linkLabel(state.link)}.
			{#if state.linkError}<span>{state.linkError}</span>{/if}
			{#if state.link === "offline"}Start the daemon, this window reconnects by itself.{/if}
		</div>
	{/if}
	{#each state.warnings as warning (warning.code + warning.message)}
		<div class="banner">{warning.message}</div>
	{/each}
	{#if error}<div class="banner error" role="alert">{error}</div>{/if}
	{#if state.notice}<div class="banner">{state.notice}</div>{/if}
</div>
