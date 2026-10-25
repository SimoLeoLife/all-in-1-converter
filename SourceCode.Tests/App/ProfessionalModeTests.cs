using Habbo_Downloader.App;
using Habbo_Downloader.App.Operations;
using Xunit;

namespace Habbo_Downloader.Tests.App;

public sealed class ProfessionalModeTests
{
    [Fact]
    public void ArgsRecognizeProfessionalMode()
    {
        Args args = Args.Parse(["--professional"]);

        Assert.Equal(RunMode.Professional, args.Mode);
        Assert.True(args.ModeExplicitlySet);
    }

    [Fact]
    public void CatalogContainsEveryExistingOperationOnce()
    {
        IReadOnlyList<OperationDefinition> operations = OperationCatalog.All;

        Assert.NotEmpty(operations);
        Assert.Equal(operations.Count, operations.Select(item => item.Id).Distinct().Count());
        // Every operation belongs to a category the sidebar and the menus know about.
        Assert.Equal(operations.Count, Enum.GetValues<OperationCategory>().Sum(category => OperationCatalog.ForCategory(category).Count));
        Assert.All(operations, item =>
        {
            Assert.False(string.IsNullOrWhiteSpace(item.Title));
            Assert.False(string.IsNullOrWhiteSpace(item.Description));
            Assert.NotNull(item.Action);
        });
    }

    // The number of operations grows with every release, so the check is that no category ever
    // loses what it had: the count each menu reached, not the count it happens to have today.
    [Theory]
    [InlineData(OperationCategory.HabboOriginal, 13)]
    [InlineData(OperationCategory.NitroCustom, 2)]
    [InlineData(OperationCategory.HotelTools, 10)]
    [InlineData(OperationCategory.Database, 5)]
    [InlineData(OperationCategory.General, 2)]
    public void CatalogPreservesAllMenuCategories(OperationCategory category, int atLeast)
    {
        Assert.True(OperationCatalog.ForCategory(category).Count >= atLeast,
            $"{category} has {OperationCatalog.ForCategory(category).Count} operations, it had {atLeast}.");
    }

    [Fact]
    public void EveryPromptDrivenOperationEnablesNativeInput()
    {
        // These ask a question while they run, so the window has to offer its input row for them.
        string[] mustAsk =
        [
            "habbo.clothes",
            "habbo.effects",
            "habbo.all",
            "nitro.furniture",
            "tools.merge-furnidata",
            "tools.merge-productdata",
            "tools.merge-clothes",
            "tools.generate-sql",
            "tools.swf-furniture-nitro",
            "tools.swf-clothes-nitro"
        ];

        HashSet<string> promptDriven = OperationCatalog.All
            .Where(operation => operation.RequiresInput)
            .Select(operation => operation.Id)
            .ToHashSet(StringComparer.Ordinal);

        Assert.All(mustAsk, id => Assert.Contains(id, promptDriven));
    }
}
