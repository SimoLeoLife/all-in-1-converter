using ConsoleApplication;
using Xunit;

namespace Habbo_Downloader.Tests.Download;

/// <summary>
/// The quest image names read out of external_flash_texts. The lines here are real ones, with the
/// key shapes the file actually uses: "quests.&lt;quest&gt;.&lt;field&gt;" and
/// "quests.&lt;campaign&gt;.&lt;quest&gt;.&lt;field&gt;".
/// </summary>
public sealed class QuestImageNameTests
{
    [Fact]
    public void AThreeSegmentKeyNamesTheQuestAndNotTheTextBehindTheEquals()
    {
        // The line from the bug report: the name used to come out as
        // "crafting_hween23_5_name=il grande creatore", and crafting_hween23_5.png - which the
        // server does have - was never asked for.
        IReadOnlyList<string> names = QuestsDownloader.ParseQuestImageNames(
            ["quests.crafting_hween23_5.name=il grande creatore"]);

        Assert.Equal(["crafting_hween23_5"], names);
    }

    [Fact]
    public void AFourSegmentKeyNamesTheCampaignAndTheCampaignWithTheQuest()
    {
        IReadOnlyList<string> names = QuestsDownloader.ParseQuestImageNames(
            ["quests.2013CNY1.1358934006765.name=Temple of the Ox"]);

        Assert.Equal(["2013CNY1_1358934006765", "2013CNY1"], names);
    }

    [Fact]
    public void NoNameEverCarriesTheTextOfItsLine()
    {
        string[] lines =
        [
            "quests.1763647832077.desc=Put on your team's badge to get started!",
            "quests.2013CNY1.1358933120103.chaincaption=Find the Ox Temple",
            "quests.crafting_hween23_5.1695031564585.completed=Congrats! Keep crafting. Win a prize!"
        ];

        IReadOnlyList<string> names = QuestsDownloader.ParseQuestImageNames(lines);

        Assert.All(names, name =>
        {
            Assert.DoesNotContain("=", name);
            Assert.DoesNotContain(" ", name);
            Assert.DoesNotContain(".", name);
        });
    }

    [Fact]
    public void EveryNameIsAskedForOnlyOnce()
    {
        // One campaign, four fields of two quests: six names at most, each one once.
        string[] lines =
        [
            "quests.2013CNY1.1358934006765.name=Temple of the Ox",
            "quests.2013CNY1.1358934006765.desc=Find the temple of the Ox in the Maze.",
            "quests.2013CNY1.1358933120103.name=Temple of the Rat",
            "quests.2013CNY1.1358933120103.desc=Find the temple of the Rat in the Maze.",
            "quests.2013CNY1.name=Temple Quest 1"
        ];

        IReadOnlyList<string> names = QuestsDownloader.ParseQuestImageNames(lines);

        Assert.Equal(["2013CNY1_1358934006765", "2013CNY1", "2013CNY1_1358933120103"], names);
        Assert.Equal(names.Count, names.Distinct(StringComparer.OrdinalIgnoreCase).Count());
    }

    [Theory]
    [InlineData("quests.name=Quests")]                 // names no quest
    [InlineData("badge_name_ACH_Login1=Login")]        // not a quest line at all
    [InlineData("")]
    public void LinesThatNameNoQuestAreSkipped(string line)
    {
        Assert.Empty(QuestsDownloader.ParseQuestImageNames([line]));
    }
}
