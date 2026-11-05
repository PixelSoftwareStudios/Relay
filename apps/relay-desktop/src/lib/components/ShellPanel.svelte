<script lang="ts">
	import { HOTKEY_NOTE } from "../transmit";
	import type { Bridge } from "../types";

	let { api, run }: { api: Bridge; run: (action: () => Promise<void>) => Promise<void> } = $props();
</script>

<section class="stack" aria-labelledby="shell-h">
	<h2 id="shell-h">This app</h2>
	<div class="card stack">
		<p>
			Closing the window hides it to the tray. Quitting this app also leaves the Relay daemon, and any call, running.
			This app cannot stop the daemon.
		</p>
		<div class="row">
			<button onclick={() => run(() => api.hideWindow())}>Hide window</button>
			<button class="danger" onclick={() => run(() => api.quitApp())}>Quit this app (daemon keeps running)</button>
		</div>
		<p class="muted">{HOTKEY_NOTE}</p>
	</div>
</section>
