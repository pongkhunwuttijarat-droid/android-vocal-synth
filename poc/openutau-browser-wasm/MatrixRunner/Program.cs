using System.Text.Json;
using OpenUtauBrowserWasmHost;

var assemblyPath = Path.Combine(AppContext.BaseDirectory, "OpenUtau.Plugin.Builtin.dll");
var outputPath = args.Length > 0
    ? Path.GetFullPath(args[0])
    : Path.GetFullPath("poc/openutau-browser-wasm/artifacts/official-phonemizer-matrix.json");
var cases = OfficialPhonemizerMatrixRunner.Run(File.ReadAllBytes(assemblyPath));
Directory.CreateDirectory(Path.GetDirectoryName(outputPath)!);
await File.WriteAllTextAsync(outputPath, JsonSerializer.Serialize(cases, new JsonSerializerOptions {
    WriteIndented = true,
}));

var cleanPassCount = cases.Count(testCase => testCase.Status == "PASS");
var outputCount = cases.Count(testCase => testCase.Status is "PASS" or "PARTIAL");
Console.WriteLine($"official cases: {cases.Count}; Process returned output: {outputCount}; clean lifecycle: {cleanPassCount}; blocked/errors: {cases.Count - outputCount}");
Console.WriteLine($"JSON: {outputPath}");
foreach (var testCase in cases) {
    var result = testCase.Output.Length > 0 ? testCase.Output : testCase.Detail;
    Console.WriteLine($"{testCase.Status,-34} {testCase.Tag,-18} {testCase.Name} :: {result}");
}
