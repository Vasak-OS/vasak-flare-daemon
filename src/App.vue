<script setup lang="ts">
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useConfigStore } from '@vasakgroup/plugin-config-manager';
import { getIconSource } from '@vasakgroup/plugin-vicons';
import { useI18n } from '@vasakgroup/tauri-plugin-i18n';
import type { Store } from 'pinia';
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';

const { t } = useI18n();

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

/**
 * Una notificación mientras está en pantalla.
 *
 * Cada cartel tiene su propio ícono ya resuelto y su propio reloj: llegan de a
 * una y se van cuando les toca, no todas juntas.
 */
interface Banner {
	notification: FlareNotification;
	icon: string;
	timer: ReturnType<typeof setTimeout> | null;
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

/** Ancho fijo de la pila. Alcanza para que los botones no pisen el texto. */
const BANNER_WIDTH = 420;

/** Techo de carteles visibles a la vez, antes de mirar el alto de la pantalla. */
const MAX_STACK = 3;

/** Alto aproximado de un cartel, sólo para estimar cuántos entran. */
const CARD_HEIGHT_HINT = 150;

/**
 * Cuántas notificaciones vivas se recuerdan.
 *
 * Las que no entran en la pila igual quedan contadas, pero la cola no puede
 * crecer sin fin: el historial completo ya está en la base del demonio.
 */
const QUEUE_LIMIT = 20;

/**
 * Cuántos carteles se apilan sin tapar media pantalla.
 *
 * La ventana está anclada abajo a la derecha y crece hacia arriba, así que el
 * límite real es el alto del monitor: en una pantalla chica se apilan menos.
 */
const maxVisible = (() => {
	const screenHeight = window.screen?.height ?? 0;
	if (!screenHeight) return MAX_STACK;
	const fits = Math.floor((screenHeight * 0.5) / CARD_HEIGHT_HINT);
	return Math.min(MAX_STACK, Math.max(1, fits));
})();

/** Las notificaciones vivas, de la más vieja a la más nueva. */
const banners = ref<Banner[]>([]);
const stack = ref<HTMLElement | null>(null);
const unlisteners: UnlistenFn[] = [];
let sizeObserver: ResizeObserver | null = null;
let lastHeight = 0;

/** Las últimas: la más nueva queda abajo, pegada a la esquina donde apareció. */
const visible = computed(() => banners.value.slice(-maxVisible));

/** Las que no entran en la pila. Se muestran como un contador arriba de todo. */
const hiddenCount = computed(() => Math.max(0, banners.value.length - maxVisible));

async function resolveIcon(name: string): Promise<string> {
	if (!name) return '';
	if (name.startsWith('/') || name.startsWith('file://')) {
		// Por el protocolo de assets, no como `file://`.
		//
		// La política de contenido no permite `file:`, así que un icono con ruta
		// absoluta —lo que manda cualquier aplicación que pase su propio
		// archivo— quedaba bloqueado y el cartel salía sin icono. Y permitir
		// `file:` en `img-src` sería peor: dejaría que cualquier archivo local
		// se cargue como imagen.
		const ruta = name.startsWith('file://') ? name.slice('file://'.length) : name;
		return convertFileSrc(ruta);
	}
	try {
		return await getIconSource(name);
	} catch {
		return '';
	}
}

/** Las acciones de una notificación, sin la que dispara el clic. */
function actionsOf(notification: FlareNotification): Action[] {
	const raw = notification.actions ?? [];
	const result: Action[] = [];
	for (let i = 0; i < raw.length; i += 2) {
		const key = raw[i];
		if (key === DEFAULT_ACTION) continue;
		result.push({ key, label: raw[i + 1] || key });
	}
	return result;
}

function hasDefaultAction(notification: FlareNotification): boolean {
	return (notification.actions ?? []).some(
		(value, index) => index % 2 === 0 && value === DEFAULT_ACTION
	);
}

function clearTimer(banner: Banner) {
	if (banner.timer) {
		clearTimeout(banner.timer);
		banner.timer = null;
	}
}

/**
 * Ajusta la ventana al alto que ocupa la pila.
 *
 * La ventana layer-shell no se estira sola: si midiera siempre lo mismo, o
 * recortaría los carteles o dejaría un rectángulo invisible que igual se come
 * los clics del escritorio.
 */
async function fitWindow() {
	if (!banners.value.length) return;
	const element = stack.value;
	if (!element) return;
	const height = Math.ceil(element.getBoundingClientRect().height);
	if (height <= 0 || height === lastHeight) return;
	lastHeight = height;
	await invoke('resize_banner', { width: BANNER_WIDTH, height }).catch((error) => {
		console.error('No se pudo ajustar el tamaño del cartel', error);
	});
}

/** Muestra u oculta la ventana según haya o no notificaciones, y la mide. */
async function syncWindow() {
	if (!banners.value.length) {
		lastHeight = 0;
		await invoke('hide_banner').catch(() => {});
		return;
	}
	await invoke('show_banner').catch(() => {});
	await nextTick();
	await fitWindow();
}

/** Saca un cartel de la pila. No avisa a nadie: eso lo deciden quienes llaman. */
async function drop(notifId: number) {
	const index = banners.value.findIndex((banner) => banner.notification.notif_id === notifId);
	if (index < 0) return;
	const [removed] = banners.value.splice(index, 1);
	clearTimer(removed);
	await syncWindow();
}

/**
 * Cierra el cartel a mano y da la notificación por leída.
 *
 * Cerrar es la forma de decir «ya la vi»: si no marcara leída, el contador del
 * escritorio seguiría avisando por algo que la persona acaba de descartar.
 */
async function dismiss(banner: Banner) {
	const { id, notif_id: notifId } = banner.notification;
	await drop(notifId);
	await invoke('dismiss_notification', { id, notifId }).catch((error) => {
		console.error('No se pudo descartar la notificación', error);
	});
}

/**
 * Ejecuta una acción y cierra el cartel.
 *
 * El cartel se va igual aunque la acción falle: dejarlo puesto después de que
 * alguien lo tocó es peor que perder la acción.
 */
async function activate(banner: Banner, actionKey: string) {
	const { id, notif_id: notifId, app_name: appName } = banner.notification;
	await drop(notifId);
	// El nombre viaja con la acción para que el escritorio pueda traer al frente
	// la aplicación que avisó: hacer clic en una notificación tiene que mostrar
	// la conversación, no sólo avisarle al programa que la tocaron.
	await invoke('activate_notification', { notifId, actionKey, id, appName }).catch((error) => {
		console.error('No se pudo ejecutar la acción de la notificación', error);
	});
}

/** El clic en el cartel: la acción por omisión si la hay, y si no, cerrarlo. */
async function clicked(banner: Banner) {
	if (hasDefaultAction(banner.notification)) {
		await activate(banner, DEFAULT_ACTION);
		return;
	}
	await dismiss(banner);
}

/** Agrega (o reemplaza, si la aplicación usó replaces_id) una notificación. */
async function push(notification: FlareNotification) {
	const icon = await resolveIcon(notification.app_icon);
	const banner: Banner = { notification, icon, timer: null };
	const index = banners.value.findIndex(
		(item) => item.notification.notif_id === notification.notif_id
	);
	if (index >= 0) {
		clearTimer(banners.value[index]);
		banners.value[index] = banner;
	} else {
		banners.value.push(banner);
		while (banners.value.length > QUEUE_LIMIT) {
			const dropped = banners.value.shift();
			if (dropped) clearTimer(dropped);
		}
	}
	// Las críticas se quedan hasta que alguien las cierre.
	if (notification.urgency < 2) {
		banner.timer = setTimeout(() => void drop(notification.notif_id), HIDE_MS);
	}
	await syncWindow();
}

onMounted(async () => {
	// Load the theme (dark/light + scheme) like the rest of VasakOS.
	try {
		const configStore = useConfigStore() as Store<
			'config',
			{ config: any; loadConfig: () => Promise<void> }
		>;
		await configStore.loadConfig();
		unlisteners.push(await listen('config-changed', () => void configStore.loadConfig()));
	} catch (error) {
		console.error('Error al cargar configuración', error);
	}

	// El alto definitivo se conoce tarde: recién cuando cargó el ícono y el
	// texto terminó de acomodarse. Medir una sola vez dejaba carteles cortados.
	if (typeof ResizeObserver !== 'undefined' && stack.value) {
		sizeObserver = new ResizeObserver(() => void fitWindow());
		sizeObserver.observe(stack.value);
	}

	unlisteners.push(
		await listen<FlareNotification>('notification://new', (event) => {
			void push(event.payload);
		})
	);
	unlisteners.push(
		await listen<number>('notification://close', (event) => {
			void drop(event.payload);
		})
	);

	// Este webview se crea recién cuando llega una notificación, así que la que
	// motivó la creación —y las que aterrizaron mientras cargábamos— están
	// encoladas en el backend, no en un evento que ya pasó. Reclamarlas es lo
	// que las muestra; sin esto, el primer cartel de cada tanda no aparecería
	// nunca. Va después de suscribirse a los eventos: al revés habría un hueco
	// entre el drenado y el listener por donde se perdería una notificación.
	try {
		const pendientes = await invoke<FlareNotification[]>('banner_ready');
		for (const notification of pendientes) {
			void push(notification);
		}
	} catch (error) {
		console.error('No se pudieron reclamar las notificaciones pendientes', error);
	}
});

onUnmounted(() => {
	for (const unlisten of unlisteners) unlisten();
	sizeObserver?.disconnect();
	for (const banner of banners.value) clearTimer(banner);
});
</script>

<template>
  <!-- La pila crece hacia arriba: la ventana está anclada abajo a la derecha, así
       que la notificación más nueva queda siempre en la misma esquina. -->
  <div ref="stack" class="fixed inset-x-0 bottom-0 flex flex-col items-stretch gap-2">
    <!-- Cuántas quedaron atrás. Sin esto, una notificación tapaba a la otra sin
         que se notara que había más de una. -->
    <div
      v-if="hiddenCount"
      class="self-center rounded-corner border border-ui-border bg-ui-bg/80 px-2 py-0.5 text-[11px] font-semibold text-tx-muted shadow-lg backdrop-blur-lg"
      :aria-label="t('banner.more').replace('{0}', String(hiddenCount))"
      :title="t('banner.more').replace('{0}', String(hiddenCount))"
    >
      +{{ hiddenCount }}
    </div>
    <div
      v-for="banner in visible"
      :key="banner.notification.notif_id"
      class="flex cursor-pointer items-start gap-3 overflow-hidden rounded-corner border border-ui-border bg-ui-bg/80 p-3 shadow-lg backdrop-blur-lg"
      @click="clicked(banner)"
    >
      <img v-if="banner.icon" :src="banner.icon" class="h-10 w-10 shrink-0" alt="" />
      <div class="min-w-0 flex-1">
        <div class="flex items-start gap-2">
          <p class="min-w-0 flex-1 truncate text-[11px] uppercase tracking-wide text-tx-muted">
            {{ banner.notification.app_name }}
          </p>
          <!-- Cerrar a mano: hasta ahora la única salida era esperar los cinco
               segundos, y las críticas no se iban nunca. -->
          <button
            class="-mr-1 -mt-1 flex h-6 w-6 shrink-0 items-center justify-center rounded-corner text-tx-muted transition-colors hover:bg-ui-surface hover:text-tx-main"
            :aria-label="t('banner.close')"
            :title="t('banner.close')"
            @click.stop="dismiss(banner)"
          >
            <svg viewBox="0 0 16 16" class="h-3.5 w-3.5" aria-hidden="true">
              <path
                d="M4.5 4.5l7 7M11.5 4.5l-7 7"
                fill="none"
                stroke="currentColor"
                stroke-width="1.75"
                stroke-linecap="round"
              />
            </svg>
          </button>
        </div>
        <p class="truncate font-semibold text-tx-main">{{ banner.notification.summary }}</p>
        <p v-if="banner.notification.body" class="mt-0.5 line-clamp-3 text-sm text-tx-muted">
          {{ banner.notification.body }}
        </p>
        <!-- Las demás acciones, como botones. Antes no había forma de llegar a
             ellas: el cartel desaparecía a los cinco segundos. -->
        <div v-if="actionsOf(banner.notification).length" class="mt-2 flex flex-wrap gap-2" @click.stop>
          <button
            v-for="action in actionsOf(banner.notification)"
            :key="action.key"
            class="rounded-corner border border-ui-border px-2 py-1 text-xs text-tx-main transition-colors hover:bg-ui-surface"
            @click="activate(banner, action.key)"
          >
            {{ action.label }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
