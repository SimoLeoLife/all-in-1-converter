using Avalonia.Media;
using Habbo_Downloader.App.Professional;
using Xunit;

namespace Habbo_Downloader.Tests.App;

/// <summary>
/// The sidebar is the one part of the window that keeps its dark colours in both themes, so its
/// contrast is checked here instead of by eye. The Pro.* brushes follow the system theme and are
/// resources, not colours, which is why they are not in this test.
/// </summary>
public sealed class ProfessionalThemeTests
{
    [Fact]
    public void SidebarTextAndCallToActionMeetAccessibleContrast()
    {
        Assert.True(ContrastRatio(ProfessionalTheme.SidebarText, ProfessionalTheme.Sidebar) >= 7.0);
        Assert.True(ContrastRatio(ProfessionalTheme.SidebarMuted, ProfessionalTheme.Sidebar) >= 4.5);
        Assert.True(ContrastRatio(ProfessionalTheme.SidebarAccent, ProfessionalTheme.Sidebar) >= 4.5);
        // The primary button is white on the brand colour.
        Assert.True(ContrastRatio(Colors.White, ProfessionalTheme.Brand) >= 4.5);
    }

    [Fact]
    public void NavigationStatesAreVisuallyDistinct()
    {
        Assert.NotEqual(ProfessionalTheme.Sidebar, ProfessionalTheme.SidebarHover);
        Assert.NotEqual(ProfessionalTheme.SidebarHover, ProfessionalTheme.SidebarSelected);
        Assert.NotEqual(ProfessionalTheme.SidebarSelected, ProfessionalTheme.SidebarAccent);
        Assert.NotEqual(ProfessionalTheme.Brand, ProfessionalTheme.BrandHover);
        Assert.NotEqual(ProfessionalTheme.BrandHover, ProfessionalTheme.BrandPressed);
    }

    private static double ContrastRatio(Color foreground, Color background)
    {
        double lighter = Math.Max(Luminance(foreground), Luminance(background));
        double darker = Math.Min(Luminance(foreground), Luminance(background));
        return (lighter + 0.05) / (darker + 0.05);
    }

    private static double Luminance(Color color) =>
        (0.2126 * Linear(color.R)) + (0.7152 * Linear(color.G)) + (0.0722 * Linear(color.B));

    private static double Linear(byte component)
    {
        double channel = component / 255.0;
        return channel <= 0.04045 ? channel / 12.92 : Math.Pow((channel + 0.055) / 1.055, 2.4);
    }
}
