// Ayudantes del arranque: la espera acotada y el reintento con backoff que usan
// `main.ts` (traducciones) y `App.vue` (configuración). Viven acá, y no en línea,
// para que la garantía que prometen —el arranque nunca queda colgado esperando a
// un backend que no contesta— se pueda probar sin montar la aplicación.

export interface RetryOptions {
	/** Cuántas veces se intenta en total, contando el primero. */
	attempts: number;
	/** Espera del primer backoff, en milisegundos; se duplica en cada vuelta. */
	baseMs: number;
	/** Tope de la espera, para que el backoff exponencial no se dispare. */
	maxMs: number;
}

/**
 * Espera a que `work` resuelva, pero no más que `deadlineMs`.
 *
 * Si vence el plazo, devuelve el control igual: `work` sigue su curso y quien
 * llama decide si le importa su resultado tardío (y se encarga de no dejar su
 * rechazo sin atender). No lanza por el plazo.
 */
export async function withDeadline(work: Promise<unknown>, deadlineMs: number): Promise<void> {
	await Promise.race([work, new Promise((resolve) => setTimeout(resolve, deadlineMs))]);
}

/**
 * Reintenta `load` hasta `attempts` veces con backoff exponencial acotado.
 *
 * No lanza: si agota los intentos, vuelve igual, porque quien llama ya acota la
 * espera total con su propio plazo y un fallo del backend no debe tumbar el
 * arranque. `onError` recibe cada fallo —con el número de intento, desde 0— para
 * poder reportarlo. El último intento no espera: no hay vuelta siguiente.
 */
export async function retry(
	load: () => Promise<void>,
	{ attempts, baseMs, maxMs }: RetryOptions,
	onError?: (error: unknown, attempt: number) => void
): Promise<void> {
	for (let attempt = 0; attempt < attempts; attempt++) {
		try {
			await load();
			return;
		} catch (error) {
			onError?.(error, attempt);
			if (attempt === attempts - 1) return;
			const wait = Math.min(baseMs * 2 ** attempt, maxMs);
			await new Promise((resolve) => setTimeout(resolve, wait));
		}
	}
}
