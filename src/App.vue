<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getIconSource } from '@vasakgroup/plugin-vicons';
import { useConfigStore } from '@vasakgroup/plugin-config-manager';
import type { Store } from 'pinia';
import { computed, onMounted, onUnmounted, ref } from 'vue';

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
	/** Pares clave/etiqueta, como los define freedesktop: [clave, texto, …]. */
	actions?: string[];
}

/** Una acción ya separada en clave y texto. */
interface Action {
	key: string;
	label: string;
}

const HIDE_MS = 5000;

/**
 * La acción que se ejecuta al hacer clic en la notificación misma.
 *
 * Es la que usan las aplicaciones para «abrí esto»: el navegador para ir a la
 * página, el cliente de correo para mostrar el mensaje. Antes el clic sólo
 * ocultaba el cartel, así que era imposible llegar a ella.
 */
const DEFAULT_ACTION = 'default';

const current = ref<FlareNotification | null>(null);
const iconSrc = ref('');
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

/** Las acciones de la notificación actual, sin la que dispara el clic. */
const actions = computed<Action[]>(() => {
	const raw = current.value?.actions ?? [];
	const result: Action[] = [];
	for (let i = 0; i < raw.length; i += 2) {
		const key = raw[i];
		if (key === DEFAULT_ACTION) continue;
		result.push({ key, label: raw[i + 1] || key });
	}
	return result;
});

const hasDefaultAction = computed(() =>
	(current.value?.actions ?? []).some((value, index) => index % 2 === 0 && value === DEFAULT_ACTION),
);

/**
 * Ejecuta una acción y cierra el cartel.
 *
 * El cartel se va igual aunque la acción falle: dejarlo puesto después de que
 * alguien lo tocó es peor que perder la acción.
 */
async function activate(actionKey: string) {
	const notifId = current.value?.notif_id;
	await hide();
	if (notifId === undefined) return;
	await invoke('activate_notification', { notifId, actionKey }).catch((error) => {
		console.error('No se pudo ejecutar la acción de la notificación', error);
	});
}

/** El clic en el cartel: la acción por omisión si la hay, y si no, cerrarlo. */
async function clicked() {
	if (hasDefaultAction.value) {
		await activate(DEFAULT_ACTION);
		return;
	}
	await hide();
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
	await invoke('hide_banner').catch(() => {});
}

async function show(n: FlareNotification) {
	current.value = n;
	await resolveIcon(n.app_icon);
	await invoke('show_banner').catch(() => {});
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
    @click="clicked"
  >
    <img v-if="iconSrc" :src="iconSrc" class="h-10 w-10 shrink-0" alt="" />
    <div class="min-w-0 flex-1">
      <p class="mb-0.5 text-[11px] uppercase tracking-wide text-tx-muted">{{ current.app_name }}</p>
      <p class="truncate font-semibold text-tx-main">{{ current.summary }}</p>
      <p v-if="current.body" class="mt-0.5 line-clamp-2 text-sm text-tx-muted">{{ current.body }}</p>
      <!-- Las demás acciones, como botones. Antes no había forma de llegar a
           ellas: el cartel desaparecía a los cinco segundos. -->
      <div v-if="actions.length" class="mt-2 flex flex-wrap gap-2" @click.stop>
        <button
          v-for="action in actions"
          :key="action.key"
          class="rounded-corner border border-ui-border px-2 py-1 text-xs text-tx-main transition-colors hover:bg-ui-surface"
          @click="activate(action.key)"
        >
          {{ action.label }}
        </button>
      </div>
    </div>
  </div>
</template>
