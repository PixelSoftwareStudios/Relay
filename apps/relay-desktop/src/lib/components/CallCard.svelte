<script lang="ts">
	import { gainPercent, isTerminal, phaseLabel } from "../format";
	import { describeTransmit } from "../transmit";
	import type { Bridge, Call, CallMeter, Peer, TransmitMode } from "../types";
	import Avatar from "./Avatar.svelte";
	import Meter from "./Meter.svelte";

	let { call, peer, meter, busy, api, run }: {
		call: Call;
		peer: Peer | undefined;
		meter: CallMeter | undefined;
		busy: boolean;
		api: Bridge;
		run: (action: () => Promise<void>) => Promise<void>;
	} = $props();

	const name = $derived(peer?.displayName ?? "Unknown peer");
	const status = $derived(describeTransmit(call));
	const active = $derived(!isTerminal(call.phase) && call.phase !== "incomingRinging");
	const talking = $derived(call.phase === "connected" && !!meter?.speaking);
	let consent = $state(false);

	$effect(() => {
		consent = call.transmit.consentGranted;
	});

	const setMode = (mode: TransmitMode) => run(() => api.setTransmit(call.callId, mode, mode === "receiveOnly" ? false : consent));
</script>

<article class="card stack" class:talking aria-label="Call with {name}">
	<div class="row">
		<Avatar {name} id={call.peerId} />
		<div class="grow">
			<h3>{name}</h3>
			<div class="muted">
				{phaseLabel(call.phase)}{talking ? " · speaking" : ""}{call.profile ? ` · ${call.profile}` : ""}
			</div>
		</div>
		{#if call.phase === "connected"}<Meter dbfs={meter?.receiveDbfs ?? null} label="Incoming level from {name}" />{/if}
	</div>

	{#if call.problem}
		<div class="banner error">{call.problem.message}{call.problem.recoveryAction ? ` ${call.problem.recoveryAction}` : ""}</div>
	{/if}

	{#if call.phase === "incomingRinging"}
		<div class="row">
			<button class="primary" disabled={busy} onclick={() => run(() => api.acceptCall(call.callId))}>Accept</button>
			<button class="danger" disabled={busy} onclick={() => run(() => api.declineCall(call.callId))}>Decline</button>
			<span class="muted">Accepting does not turn on your microphone.</span>
		</div>
	{:else if active}
		<div class="row">
			<button class="danger" disabled={busy} onclick={() => run(() => api.endCall(call.callId))}>
				{call.phase === "outgoingRinging" ? "Cancel" : "End call"}
			</button>
			<button aria-pressed={call.userMuted} disabled={busy} onclick={() => run(() => api.setCallControls(call.callId, { muted: !call.userMuted }))}>
				{call.userMuted ? "Unmute" : "Mute"} (call)
			</button>
			<button aria-pressed={call.userDeafened} disabled={busy} onclick={() => run(() => api.setCallControls(call.callId, { deafened: !call.userDeafened }))}>
				{call.userDeafened ? "Undeafen" : "Deafen"} (call)
			</button>
		</div>
		<label class="row">
			<span>Their volume {gainPercent(call.receiveGain)}</span>
			<input type="range" min="0" max="2" step="0.05" value={call.receiveGain} disabled={busy}
				onchange={(e) => run(() => api.setCallControls(call.callId, { receiveGain: Number(e.currentTarget.value) }))} />
		</label>

		<fieldset class="stack">
			<legend>Your microphone in this call</legend>
			<label class="row">
				<input type="checkbox" bind:checked={consent} disabled={busy || call.transmit.mode !== "receiveOnly"} />
				I allow my microphone to be sent to {name}
			</label>
			<div class="row" role="radiogroup" aria-label="Transmit mode">
				{#each [["receiveOnly", "Listen only"], ["pushToTalk", "Push-to-talk"], ["openMic", "Open mic"]] as [mode, label] (mode)}
					<button
						role="radio"
						aria-checked={call.transmit.mode === mode}
						class:primary={call.transmit.mode === mode}
						disabled={busy || (mode !== "receiveOnly" && !consent)}
						onclick={() => setMode(mode as TransmitMode)}>{label}</button
					>
				{/each}
			</div>
			<div class="tone-{status.tone}" aria-live="polite"><strong>{status.label}.</strong> {status.detail}</div>
		</fieldset>
	{/if}
</article>

<style>
	.talking { border-color: var(--live); }
	fieldset { border: 1px solid var(--border); border-radius: 8px; margin: 0; }
</style>
