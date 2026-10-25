using System.Text.Json;
using Habbo_Downloader.Update;
using Xunit;

namespace Habbo_Downloader.Tests.Update;

/// <summary>
/// The two decisions the startup update check makes on its own: which tag counts as a version, and
/// which file of a release can replace the running program.
/// </summary>
public sealed class AppUpdateTests
{
    [Theory]
    [InlineData("v2026.10.25.33", "2026.10.25.33")]
    [InlineData("2.1.0", "2.1.0")]
    [InlineData("release-1.2", "1.2")]
    public void ReleaseTagsBecomeVersions(string tag, string expected)
    {
        Assert.Equal(Version.Parse(expected), AppUpdate.ParseVersion(tag));
    }

    [Theory]
    [InlineData("nightly")]
    [InlineData("v2")]       // no dotted number: not a version to compare with
    [InlineData("")]
    public void TagsWithoutAVersionAreIgnored(string tag)
    {
        Assert.Null(AppUpdate.ParseVersion(tag));
    }

    [Fact]
    public void TheBareProgramIsPickedAndTheArchivesAreSkipped()
    {
        using JsonDocument release = Release(
            "all-in-1-converter-win-x64.zip",
            "all-in-1-converter-linux-x64.tar.gz",
            "All-in-1 Converter win-x64.exe",
            "All-in-1 Converter linux-x64");

        (string Name, string Url, long Bytes)? asset = AppUpdate.Asset(release.RootElement);

        Assert.NotNull(asset);
        Assert.Equal(OperatingSystem.IsWindows() ? "All-in-1 Converter win-x64.exe" : "All-in-1 Converter linux-x64",
            asset!.Value.Name);
    }

    [Fact]
    public void AReleaseOfArchivesOnlyHasNothingToUpdateWith()
    {
        // Every release before the single file was published looks like this. Offering the .zip would
        // put an archive in the place of the program, so there has to be nothing to update.
        using JsonDocument release = Release("all-in-1-converter-win-x64.zip", "all-in-1-converter-linux-x64.tar.gz");

        Assert.Null(AppUpdate.Asset(release.RootElement));
    }

    private static JsonDocument Release(params string[] assetNames)
    {
        var assets = assetNames.Select(name => new
        {
            name,
            browser_download_url = $"https://example.invalid/{Uri.EscapeDataString(name)}",
            size = 1234L
        });
        return JsonDocument.Parse(JsonSerializer.Serialize(new { assets }));
    }
}
