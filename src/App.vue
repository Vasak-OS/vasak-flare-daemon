<script setup lang="ts">
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { getIconSource } from '@vasakgroup/plugin-vicons';
import { useConfigStore } from '@vasakgroup/plugin-config-manager';
import type { Store } from 'pinia';
import { onMounted, onUnmounted, ref } from 'vue';

interface FlareNotification {
	id: number;
	notif_id: number;
	app_name: string;
	app_icon: string;
	summary: string;
	body: string;
	urgency: number;
	created_at: number;
	read: boolean;
}

const HIDE_MS = 5000;

const current = ref<FlareNotification | null>(null);
const iconSrc = ref('');
const appWindow = getCurrentWindow();
const unlisteners: UnlistenFn[] = [];
let hideTimer: ReturnType<typeof setTimeout> | null = null;

async function resolveIcon(name: string) {
	iconSrc.value = '';
	if (!name) return;
	if (name.startsWith('/') || name.startsWith('file://')) {
		iconSrc.value = name.startsWith('file://') ? name : `file://${name}`;
		return;
	}
	try {
		iconSrc.value = await getIconSource(name);
	} catch {
		iconSrc.value = '';
	}
}

function clearHide() {
	if (hideTimer) {
		clearTimeout(hideTimer);
		hideTimer = null;
	}
}

async function hide() {
	clearHide();
	current.value = null;
	await appWindow.hide();
}

async function show(n: FlareNotification) {
	current.value = n;
	await resolveIcon(n.app_icon);
	await appWindow.show();
	clearHide();
	// Critical notifications stay until dismissed.
	if (n.urgency < 2) {
		hideTimer = setTimeout(() => void hide(), HIDE_MS);
	}
}

onMounted(async () => {
	// Load the theme (dark/light + scheme) like the rest of VasakOS.
	try {
		const configStore = useConfigStore() as Store<'config', { config: any; loadConfig: () => Promise<void> }>;
		await configStore.loadConfig();
		unlisteners.push(await listen('config-changed', () => void configStore.loadConfig()));
	} catch (error) {
		console.error('Error al cargar configuración', error);
	}

	unlisteners.push(
		await listen<FlareNotification>('notification://new', (event) => {
			void show(event.payload);
		})
	);
	unlisteners.push(
		await listen<number>('notification://close', (event) => {
			if (current.value && current.value.notif_id === event.payload) void hide();
		})
	);
});

onUnmounted(() => {
	unlisteners.forEach((u) => u());
	clearHide();
});
</script>

<template>
  <div
    v-if="current"
    class="flex h-screen cursor-pointer items-start gap-3 overflow-hidden rounded-corner bg-ui-bg/95 p-3 shadow-xl"
    @click="hide"
  >
    <img v-if="iconSrc" :src="iconSrc" class="h-10 w-10 shrink-0" alt="" />
    <div class="min-w-0 flex-1">
      <p class="mb-0.5 text-[11px] uppercase tracking-wide text-tx-muted">{{ current.app_name }}</p>
      <p class="truncate font-semibold text-tx-main">{{ current.summary }}</p>
      <p v-if="current.body" class="mt-0.5 line-clamp-2 text-sm text-tx-muted">{{ current.body }}</p>
    </div>
  </div>
</template>
