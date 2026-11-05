<script lang="ts">
	import type { AppState, Bridge, Device } from "../types";

	let { state, busy, api, run, micDbfs }: {
		state: AppState;
		busy: boolean;
		api: Bridge;
		run: (action: () => Promise<void>) => Promise<void>;
		micDbfs: number | null;
	} = $props();
	import Meter from "./Meter.svelte";

	const settings = $derived(state.settings);
	const inputs = $derived(state.devices.filter((d) => d.direction === "input"));
	const outputs = $derived(state.devices.filter((d) => d.direction === "output"));
	const label = (d: Device) => `${d.name}${d.systemDefault ? " (system default)" : ""}${d.available ? "" : " (unavailable)"}`;
</script>

<section class="stack" aria-labelledby="dev-h">
	<h2 id="dev-h">Audio</h2>
	{#if settings}
		<div class="card stack">
			<label class="row">
				<span class="grow">Microphone</span>
				<select value={settings.inputDeviceId} disabled={busy} onchange={(e) => run(() => api.selectDevices(e.currentTarget.value, settings.outputDeviceId))}>
					{#each inputs as d (d.deviceId)}<option value={d.deviceId} disabled={!d.available}>{label(d)}</option>{/each}
				</select>
			</label>
			<label class="row">
				<span class="grow">Speakers</span>
				<select value={settings.outputDeviceId} disabled={busy} onchange={(e) => run(() => api.selectDevices(settings.inputDeviceId, e.currentTarget.value))}>
					{#each outputs as d (d.deviceId)}<option value={d.deviceId} disabled={!d.available}>{label(d)}</option>{/each}
				</select>
			</label>
			<div class="row"><span class="grow">Microphone level</span><Meter dbfs={micDbfs} label="Microphone level" /></div>
			<label class="row">
				<span class="grow">Microphone gain</span>
				<input type="range" min="0" max="2" step="0.05" value={settings.microphoneGain} disabled={busy}
					onchange={(e) => run(() => api.setGlobalControls({ microphoneGain: Number(e.currentTarget.value) }))} />
			</label>
			<label class="row">
				<span class="grow">Playback gain</span>
				<input type="range" min="0" max="2" step="0.05" value={settings.playbackGain} disabled={busy}
					onchange={(e) => run(() => api.setGlobalControls({ playbackGain: Number(e.currentTarget.value) }))} />
			</label>
			<div class="row">
				<button aria-pressed={settings.globalMuted} disabled={busy} onclick={() => run(() => api.setGlobalControls({ muted: !settings.globalMuted }))}>
					{settings.globalMuted ? "Unmute all" : "Mute all"}
				</button>
				<button aria-pressed={settings.globalDeafened} disabled={busy} onclick={() => run(() => api.setGlobalControls({ deafened: !settings.globalDeafened }))}>
					{settings.globalDeafened ? "Undeafen all" : "Deafen all"}
				</button>
			</div>
		</div>
	{:else}
		<p class="muted">Audio settings appear once the daemon is connected.</p>
	{/if}
</section>
