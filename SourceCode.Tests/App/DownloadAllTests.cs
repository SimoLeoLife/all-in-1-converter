using Habbo_Downloader.App.Operations;
using Xunit;

namespace Habbo_Downloader.Tests.App;

/// <summary>
/// "Download all" ran eight of the fourteen downloads of the Habbo Original menu for a long time,
/// because every download added to the menu had to be remembered in a second place. These hold the
/// two together.
/// </summary>
public sealed class DownloadAllTests
{
    [Fact]
    public void DownloadAllRunsOneStepForEveryDownloadInTheMenu()
    {
        // Every operation of the menu except "Download all" itself is one step of the sequence.
        int downloads = OperationCatalog.ForCategory(OperationCategory.HabboOriginal).Count - 1;

        Assert.Equal(downloads, OperationCatalog.DownloadAllSequence.Count);
    }

    [Fact]
    public void EveryStepHasANameAndSomethingToRun()
    {
        Assert.All(OperationCatalog.DownloadAllSequence, step =>
        {
            Assert.False(string.IsNullOrWhiteSpace(step.Name));
            Assert.NotNull(step.Run);
        });
        Assert.Equal(OperationCatalog.DownloadAllSequence.Count,
            OperationCatalog.DownloadAllSequence.Select(step => step.Name).Distinct().Count());
    }

    [Fact]
    public void TheMenuStillOffersDownloadAll()
    {
        OperationDefinition all = OperationCatalog.Get("habbo.all");

        Assert.Equal(OperationCategory.HabboOriginal, all.Category);
        // It asks the questions of the downloads it runs, so the window has to offer its input row.
        Assert.True(all.RequiresInput);
    }
}
