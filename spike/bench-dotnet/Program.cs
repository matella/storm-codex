// Jalon 0 — baseline Heroes.StormReplayParser (.NET). Mêmes règles que bench_python.py :
// mono-thread, in-process, warm-up exclu, un échec n'arrête pas le bench.
using System.Diagnostics;
using System.Globalization;
using Heroes.StormReplayParser;

var root = AppContext.BaseDirectory;
// remonte jusqu'à la racine du repo (présence de corpus/spike50)
var dir = new DirectoryInfo(root);
while (dir is not null && !Directory.Exists(Path.Combine(dir.FullName, "corpus", "spike50")))
    dir = dir.Parent;
if (dir is null) { Console.Error.WriteLine("corpus/spike50 introuvable"); return 1; }

var corpus = Path.Combine(dir.FullName, "corpus", "spike50");
var outDir = Path.Combine(dir.FullName, "spike", "bench-results");
Directory.CreateDirectory(outDir);

var files = Directory.GetFiles(corpus, "*.StormReplay").OrderBy(f => f).ToArray();
if (files.Length != 50) { Console.Error.WriteLine($"corpus inattendu : {files.Length} fichiers"); return 1; }

// périmètre aligné sur le bench Python : header + details + tracker events
// (pas de game/message events — le spike ne les mesure pour aucun moteur)
var options = new ParseOptions
{
    ShouldParseTrackerEvents = true,
    ShouldParseGameEvents = false,
    ShouldParseMessageEvents = false,
};

StormReplay.Parse(files[0], options); // warm-up (JIT), mesure jetée

var rows = new List<string> { "name,ms,build,ok" };
var times = new List<double>();
int fails = 0;
foreach (var f in files)
{
    var sw = Stopwatch.StartNew();
    int build = 0, ok = 1;
    try
    {
        var result = StormReplay.Parse(f, options);
        if (result.Status != StormReplayParseStatus.Success)
            throw new InvalidOperationException($"status={result.Status}");
        build = result.Replay.ReplayBuild;
    }
    catch (Exception e)
    {
        ok = 0; fails++;
        Console.WriteLine($"FAIL {Path.GetFileName(f)}: {e.GetType().Name}: {e.Message}");
    }
    sw.Stop();
    var ms = sw.Elapsed.TotalMilliseconds;
    if (ok == 1) times.Add(ms);
    rows.Add(string.Create(CultureInfo.InvariantCulture,
        $"{Path.GetFileName(f)},{ms:F1},{build},{ok}"));
}

File.WriteAllLines(Path.Combine(outDir, "dotnet.csv"), rows);
times.Sort();
double median = times[times.Count / 2];
double p95 = times[(int)Math.Ceiling(0.95 * times.Count) - 1];
Console.WriteLine(string.Create(CultureInfo.InvariantCulture,
    $"dotnet : n={times.Count} échecs={fails} médiane={median:F0} ms p95={p95:F0} ms max={times[^1]:F0} ms"));
return 0;
