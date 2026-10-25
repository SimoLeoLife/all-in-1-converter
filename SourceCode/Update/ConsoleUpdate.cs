namespace Habbo_Downloader.Update;

/// <summary>
/// The same update as the Professional dialog, at the console: the CLI menu asks before it opens, and a
/// scripted run (--op, --command, --benchmark, redirected input) only gets a line saying a newer version
/// exists. Nothing here ever stops the program from starting.
/// </summary>
public static class ConsoleUpdate
{
    /// <summary>
    /// True when the program was replaced and the caller should start the new one and leave. <paramref
    /// name="ask"/> false only prints the notice: a run that nobody watches must not wait for an answer.
    /// </summary>
    public static async Task<bool> OfferAsync(bool ask, CancellationToken token = default)
    {
        AvailableUpdate? found = await AppUpdate.CheckAsync(token);
        if (found is not { } update) return false;

        Console.ForegroundColor = ConsoleColor.Yellow;
        Console.WriteLine($"Version {update.Version} of the converter is available (you run {AppUpdate.Current}).");
        Console.ResetColor();

        // An answer can only be read from a console somebody is sitting at. --no-update is not the
        // reason either way: with that flag there was no check to begin with.
        if (!ask || Console.IsInputRedirected)
        {
            Console.WriteLine($"Download it from {update.PageUrl}.");
            Console.WriteLine(Console.IsInputRedirected
                ? "It installs itself when the converter is started in a terminal; with the input redirected it cannot ask."
                : "Open the menu without --op / --command / --benchmark and it offers to install it by itself.");
            Console.WriteLine();
            return false;
        }

        Console.Write("Download it now and restart? [Y/n] ");
        string answer = (Console.ReadLine() ?? "").Trim();
        if (answer.Length > 0 && !answer.StartsWith("y", StringComparison.OrdinalIgnoreCase))
        {
            Console.WriteLine($"Skipped. The release stays at {update.PageUrl}.");
            Console.WriteLine();
            return false;
        }

        try
        {
            await AppUpdate.DownloadAndReplaceAsync(update, Report(update.AssetName), token);
            Console.WriteLine();
            Console.ForegroundColor = ConsoleColor.Green;
            Console.WriteLine("Downloaded. Starting the new version ...");
            Console.ResetColor();
            return true;
        }
        catch (Exception ex)
        {
            Console.WriteLine();
            Console.ForegroundColor = ConsoleColor.Red;
            Console.WriteLine($"The update failed: {ex.Message}");
            Console.WriteLine($"Download it yourself from {update.PageUrl}. This copy keeps working.");
            Console.ResetColor();
            Console.WriteLine();
            return false;
        }
    }

    // One line that rewrites itself, so a 70 MB download does not scroll the menu away.
    private static IProgress<(long Done, long Total)> Report(string asset)
    {
        long last = -1;
        return new Progress<(long Done, long Total)>(p =>
        {
            long percent = p.Total > 0 ? p.Done * 100 / p.Total : -1;
            if (percent == last) return;
            last = percent;
            string size = p.Total > 0 ? $"{Megabytes(p.Done)} of {Megabytes(p.Total)} ({percent}%)" : Megabytes(p.Done);
            Console.Write($"\rDownloading {asset}: {size}   ");
        });
    }

    private static string Megabytes(long bytes) => bytes <= 0 ? "?" : $"{bytes / (1024.0 * 1024.0):0.0} MB";
}
