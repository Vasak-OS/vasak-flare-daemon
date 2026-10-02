<script setup lang="ts">
/**
 * Un cartel de notificación: la tarjeta que aparece abajo a la derecha.
 *
 * # La superficie
 *
 * `bg-ui-shell` y **sin `backdrop-blur`**. La ventana de los carteles es una
 * superficie de capa transparente: lo que tiene detrás es el escritorio, y el
 * desenfoque lo pone Wayfire (decisión del usuario del 02/10/2026, §13 de
 * `docs/once-ui.md` en vue-libvasak). El `bg-ui-bg/80 backdrop-blur-lg` de
 * antes tenía los dos problemas: el desenfoque del WebView no veía el
 * escritorio, y al 80 % el texto principal sobre un fondo de pantalla negro
 * daba 4,42:1, por debajo del 4,5 que pide WCAG. `ui-shell` es el fondo de la
 * ventana al 85 %, y ahí da 5,01:1 en el peor caso. Lo miden
 * `tests/translucent-surfaces.test.ts` y `tests/shell-contrast.test.ts`.
 *
 * # La forma
 *
 * La tarjeta de Once UI: `rounded-corner-l`, canto `ui-line` y la sombra de
 * tres capas con la tinta del esquema. El tamaño, la posición y lo que dice
 * son los de siempre: el icono de 40, el nombre de la aplicación, el título,
 * hasta tres líneas de cuerpo y las acciones.
 *
 * # El texto, todo en `tx-main`
 *
 * El nombre de la aplicación y el cuerpo decían `text-tx-muted`, pero nunca se
 * vieron así: el `body * { @apply text-tx-main }` del `main.css` de antes iba
 * fuera de toda capa y le ganaba a cualquier utilidad. Ahora que el color por
 * omisión va en la capa `base`, `tx-muted` se aplicaría de verdad, y sobre la
 * superficie translúcida no llega: con el esquema de fábrica en claro y un
 * fondo de pantalla negro da 3,10:1. Detrás de un cartel puede haber cualquier
 * fondo, así que el texto va en `tx-main` (5,01:1 en ese mismo caso), que es
 * además como se veía. La jerarquía la dan el tamaño, el peso y las mayúsculas.
 * Lo mide `tests/shell-contrast.test.ts` con cada clase `text-tx-*` que use
 * este archivo.
 *
 * # El ancho
 *
 * La pila mide lo que mida la ventana (420 px hoy), así que la tarjeta se
 * adapta a su propio ancho con una consulta de contenedor y no a la pantalla:
 * por debajo de 16rem el icono baja a 32 y el espacio se achica, para que el
 * texto no quede en una columna de tres palabras.
 */

import { useI18n } from '@vasakgroup/tauri-plugin-i18n';
import { ActionButton, ThemeIcon } from '@vasakgroup/vue-libvasak';
import { computed } from 'vue';
import {
	type BannerIcon,
	type FlareNotification,
	hasDefaultAction,
	parseActions,
} from '@/tools/notification';

const props = defineProps<{
	notification: FlareNotification;
	/** El icono ya separado en nombre del tema o archivo propio de la aplicación. */
	icon: BannerIcon | null;
}>();

const emit = defineEmits<{
	/** Clic en la tarjeta: la acción por omisión, o cerrarla. */
	activate: [];
	/** El botón de cerrar. */
	dismiss: [];
	/** Una de las acciones con botón. */
	action: [key: string];
}>();

const { t } = useI18n();

const actions = computed(() => parseActions(props.notification));
const clickable = computed(() => hasDefaultAction(props.notification));
</script>

<template>
  <article
    class="@container cursor-pointer overflow-hidden rounded-corner-l border border-ui-line bg-ui-shell p-3 text-tx-main shadow-surface-m"
    :role="clickable ? 'button' : undefined"
    :tabindex="clickable ? 0 : undefined"
    :data-urgency="notification.urgency"
    @click="emit('activate')"
    @keydown.enter.self.prevent="emit('activate')"
    @keydown.space.self.prevent="emit('activate')">
    <!-- La consulta de contenedor mira a la tarjeta, así que lo que cambia con el
         ancho va un nivel adentro: un contenedor no se consulta a sí mismo. -->
    <div class="flex items-start gap-3 @max-[16rem]:gap-2">
      <ThemeIcon
        v-if="icon"
        :name="icon.name"
        :fallbacks="icon.fallbacks"
        :fallback-src="icon.src"
        size="auto"
        class="size-10 @max-[16rem]:size-8" />
      <div class="min-w-0 flex-1">
        <div class="flex items-start gap-2">
          <p class="min-w-0 flex-1 truncate text-label-xs uppercase tracking-wide text-tx-main">
            {{ notification.app_name }}
          </p>
          <!-- Cerrar a mano: hasta ahora la única salida era esperar los cinco
               segundos, y las críticas no se iban nunca. El botón de 24 de la
               librería mide 26 con su canto, y el `-mb-0.5` devuelve esos dos
               píxeles: la fila queda del alto de antes y el cartel no crece. -->
          <ActionButton
            label=""
            icon="window-close-symbolic"
            :icon-alt="t('banner.close')"
            :title="t('banner.close')"
            variant="ghost"
            size="sm"
            stop-propagation
            custom-class="-mr-1 -mt-1 -mb-0.5 shrink-0"
            @click="emit('dismiss')" />
        </div>
        <p class="truncate font-title text-body-m font-semibold text-tx-main">{{ notification.summary }}</p>
        <p v-if="notification.body" class="mt-0.5 line-clamp-3 text-body-s text-tx-main">
          {{ notification.body }}
        </p>
        <!-- Las demás acciones, como botones. Antes no había forma de llegar a
             ellas: el cartel desaparecía a los cinco segundos. Cada botón corta
             su propio clic (`stop-propagation`) para no disparar además el de
             la tarjeta; la fila no escucha nada. -->
        <div v-if="actions.length" class="mt-2 flex flex-wrap gap-2">
          <ActionButton
            v-for="action in actions"
            :key="action.key"
            :label="action.label"
            variant="secondary"
            size="sm"
            stop-propagation
            @click="emit('action', action.key)" />
        </div>
      </div>
    </div>
  </article>
</template>
