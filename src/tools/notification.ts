/**
 * Lo que el cartel necesita saber de una notificación, sin Vue ni Tauri en el
 * medio: así se prueba solo.
 */

export interface FlareNotification {
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
export interface NotificationAction {
	key: string;
	label: string;
}

/**
 * El icono de un cartel, listo para `ThemeIcon`.
 *
 * `app_icon` trae una de dos cosas: un **nombre del tema** (`mail-unread`) o la
 * **ruta de un archivo** que manda la aplicación con su propio dibujo. El
 * nombre va a `name`, con un respaldo genérico por si el tema no lo tiene; la
 * ruta va a `src`, que `ThemeIcon` dibuja sólo cuando ningún nombre resolvió.
 * La ruta ya viene pasada por el protocolo de assets (ver `iconFor`).
 */
export interface BannerIcon {
	name: string;
	fallbacks: string[];
	src: string;
}

/**
 * La acción que se ejecuta al hacer clic en la notificación misma.
 *
 * Es la que usan las aplicaciones para «abrí esto»: el navegador para ir a la
 * página, el cliente de correo para mostrar el mensaje. Antes el clic sólo
 * ocultaba el cartel, así que era imposible llegar a ella.
 */
export const DEFAULT_ACTION = 'default';

/** El icono genérico de una aplicación, para un nombre que el tema no tiene. */
export const GENERIC_APP_ICON = 'application-x-executable';

/** Las acciones de una notificación, sin la que dispara el clic. */
export function parseActions(notification: FlareNotification): NotificationAction[] {
	const raw = notification.actions ?? [];
	const result: NotificationAction[] = [];
	for (let i = 0; i < raw.length; i += 2) {
		const key = raw[i] as string;
		if (key === DEFAULT_ACTION) continue;
		result.push({ key, label: raw[i + 1] || key });
	}
	return result;
}

export function hasDefaultAction(notification: FlareNotification): boolean {
	return (notification.actions ?? []).some(
		(value, index) => index % 2 === 0 && value === DEFAULT_ACTION
	);
}

/**
 * El icono de un `app_icon`, o `null` si la notificación no trae ninguno.
 *
 * Una ruta absoluta va por el protocolo de assets (`toAssetUrl`, que es
 * `convertFileSrc`) y no como `file://`: la política de contenido no permite
 * `file:`, así que un icono con ruta —lo que manda cualquier aplicación que
 * pase su propio archivo— quedaba bloqueado y el cartel salía sin icono. Y
 * permitir `file:` en `img-src` sería peor: dejaría que cualquier archivo
 * local se cargue como imagen.
 */
export function iconFor(appIcon: string, toAssetUrl: (path: string) => string): BannerIcon | null {
	if (!appIcon) return null;
	if (appIcon.startsWith('/')) {
		return { name: '', fallbacks: [], src: toAssetUrl(appIcon) };
	}
	if (appIcon.startsWith('file://')) {
		const path = filePath(appIcon);
		return path ? { name: '', fallbacks: [], src: toAssetUrl(path) } : null;
	}
	return { name: appIcon, fallbacks: [GENERIC_APP_ICON], src: '' };
}

/**
 * La ruta local de una URI `file://`, o `null` si no es de esta máquina o está
 * mal formada.
 *
 * Una URI va codificada: `file:///home/ana/mi%20icono.png` nombra
 * `mi icono.png`. Cortar el esquema a mano dejaba el `%20`, y `convertFileSrc`
 * lo volvía a codificar, así que el protocolo de assets buscaba un archivo con
 * `%20` en el nombre y el cartel salía sin icono. `file://localhost/…` es la
 * misma máquina; con otro host no hay archivo local que mostrar.
 */
function filePath(uri: string): string | null {
	try {
		const url = new URL(uri);
		if (url.hostname && url.hostname !== 'localhost') return null;
		return decodeURIComponent(url.pathname) || null;
	} catch {
		return null;
	}
}
