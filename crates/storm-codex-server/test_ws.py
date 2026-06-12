# Test WS jalon 3 T5 : connecte /ws, déclenche un upload, mesure le délai jusqu'à match.parsed.
import asyncio, json, subprocess, sys, time, glob
import websockets

REPLAY = glob.glob(str(__import__('pathlib').Path.home() / "Desktop/Coding/storm-codex/corpus/stats/*Cursed Hollow*.StormReplay"))[0]

async def main():
    async with websockets.connect("ws://127.0.0.1:8088/ws") as ws:
        await asyncio.sleep(0.3)  # abonnement établi
        t0 = time.perf_counter()
        # upload via curl en parallèle
        subprocess.run(["curl", "-s", "-X", "POST", "http://127.0.0.1:8088/api/upload",
                        "-H", "Authorization: Bearer devtoken", "--data-binary", f"@{REPLAY}"],
                       capture_output=True)
        try:
            msg = await asyncio.wait_for(ws.recv(), timeout=5)
        except asyncio.TimeoutError:
            print("ÉCHEC : pas d'event WS en 5 s"); sys.exit(1)
        dt = (time.perf_counter() - t0) * 1000
        ev = json.loads(msg)
        print(f"event reçu en {dt:.0f} ms : {ev}")
        assert ev.get("type") == "match.parsed", f"type inattendu : {ev}"
        assert dt < 5000, "dépasse 5 s"
        print("OK : match.parsed reçu < 5 s")

asyncio.run(main())
