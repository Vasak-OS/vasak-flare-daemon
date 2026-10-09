import { describe, expect, test } from 'bun:test';
import { retry, withDeadline } from '../src/tools/startup';

/** Una promesa que resuelve recién a los `ms`, para simular un backend lento. */
const demora = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

describe('withDeadline', () => {
	test('vuelve apenas termina el trabajo si llega antes del plazo', async () => {
		let listo = false;
		const trabajo = demora(5).then(() => {
			listo = true;
		});
		await withDeadline(trabajo, 1000);
		expect(listo).toBe(true);
	});

	test('vuelve igual cuando vence el plazo y el trabajo sigue colgado', async () => {
		// El trabajo no resuelve nunca dentro de la prueba: lo que se comprueba es
		// que `withDeadline` no se cuelga con él.
		const colgado = new Promise<void>(() => {});
		const inicio = Date.now();
		await withDeadline(colgado, 10);
		expect(Date.now() - inicio).toBeGreaterThanOrEqual(9);
	});

	test('no lanza aunque el trabajo rechace después de vencer el plazo', async () => {
		const fallo = demora(10).then(() => {
			throw new Error('tarde');
		});
		// Quien llama atiende el rechazo tardío; acá sólo importa que el plazo gane
		// sin propagar el error.
		fallo.catch(() => {});
		await expect(withDeadline(fallo, 1)).resolves.toBeUndefined();
	});
});

describe('retry', () => {
	test('no reintenta si el primer intento sale bien', async () => {
		let llamadas = 0;
		await retry(
			async () => {
				llamadas++;
			},
			{ attempts: 3, baseMs: 1, maxMs: 2 }
		);
		expect(llamadas).toBe(1);
	});

	test('reintenta y termina bien cuando un intento intermedio funciona', async () => {
		let llamadas = 0;
		await retry(
			async () => {
				llamadas++;
				if (llamadas < 2) throw new Error('todavía no');
			},
			{ attempts: 3, baseMs: 1, maxMs: 2 }
		);
		expect(llamadas).toBe(2);
	});

	test('agota los intentos y vuelve sin lanzar', async () => {
		let llamadas = 0;
		await expect(
			retry(
				async () => {
					llamadas++;
					throw new Error('siempre falla');
				},
				{ attempts: 3, baseMs: 1, maxMs: 2 }
			)
		).resolves.toBeUndefined();
		expect(llamadas).toBe(3);
	});

	test('avisa de cada fallo con el número de intento desde 0', async () => {
		const intentos: number[] = [];
		await retry(
			async () => {
				throw new Error('nop');
			},
			{ attempts: 3, baseMs: 1, maxMs: 2 },
			(_error, intento) => intentos.push(intento)
		);
		expect(intentos).toEqual([0, 1, 2]);
	});

	test('el último intento no espera: el backoff sólo va entre vueltas', async () => {
		// Con 3 intentos hay a lo sumo 2 esperas (base y base*2, acotadas a maxMs).
		// La prueba fija maxMs chico para que la cota se note y no dependa del reloj.
		const inicio = Date.now();
		await retry(
			async () => {
				throw new Error('nop');
			},
			{ attempts: 3, baseMs: 5, maxMs: 8 }
		);
		const transcurrido = Date.now() - inicio;
		// Dos esperas: 5 ms y min(10, 8)=8 ms ≈ 13 ms; sin la cota serían 5+10=15.
		expect(transcurrido).toBeGreaterThanOrEqual(12);
		expect(transcurrido).toBeLessThan(100);
	});
});
