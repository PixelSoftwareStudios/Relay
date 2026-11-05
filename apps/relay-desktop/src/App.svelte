<script lang="ts">
	import { onMount } from "svelte";
	import logo from "$assets/relay-mark.svg";
	import { selectBridge } from "./lib/bridge";
	import CallCard from "./lib/components/CallCard.svelte";
	import DevicePanel from "./lib/components/DevicePanel.svelte";
	import PeerList from "./lib/components/PeerList.svelte";
	import ProfilePanel from "./lib/components/ProfilePanel.svelte";
	import ShellPanel from "./lib/components/ShellPanel.svelte";
	import StatusBar from "./lib/components/StatusBar.svelte";
	import { RelayStore } from "./lib/store.svelte";

	const store = new RelayStore(selectBridge());
	const api = store.bridgeApi;
	const run = (action: () => Promise<void>) => store.run(action);
	const liveCalls = $derived(store.state.calls.filter((c) => !["ended", "failed", "declined", "cancelled"].includes(c.phase)));

	onMount(() => {
		void store.start();
		return () => store.stop();
	});
</script>

<main>
	<header class="row">
		<img src={logo} alt="" width="32" height="32" />
		<h1 class="grow">Relay</h1>
		{#if store.state.engine}<span class="muted">engine {store.state.engine.version}</span>{/if}
	</header>

	<StatusBar state={store.state} error={store.error} />

	<section class="stack" aria-labelledby="calls-h">
		<h2 id="calls-h">Calls</h2>
		{#each liveCalls as call (call.callId)}
			<CallCard {call} peer={store.state.peers.find((p) => p.peerId === call.peerId)} meter={store.callMeter(call.callId)} busy={store.busy} {api} {run} />
		{:else}
			<p class="muted">No active calls.</p>
		{/each}
	</section>

	<PeerList state={store.state} busy={store.busy} onCall={(id) => run(() => api.startCall(id))} />
	<DevicePanel state={store.state} busy={store.busy} {api} {run} micDbfs={store.meters.microphoneDbfs} />
	<ProfilePanel busy={store.busy} {api} {run} />
	<ShellPanel {api} {run} />
</main>

<style>
	main { max-width: 720px; margin: 0 auto; padding: 16px; display: flex; flex-direction: column; gap: 18px; }
	h1 { font-size: 20px; }
</style>
