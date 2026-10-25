namespace ConsoleApplication
{
    public static class QuestsDownloader
    {
        private static readonly HttpClient httpClient = new HttpClient();
        private const int MaxParallelDownloads = 8;
        private const string QuestsDirectory = "Habbo_Default/quests";

        public static async Task DownloadQuestsAsync()
        {
            string configFilePath = "config.ini";
            var config = IniFileParser.Parse(configFilePath);
            string tempFilePath = "./temp/external_texts.txt";

            try
            {
                Directory.CreateDirectory("./temp");
                Directory.CreateDirectory(QuestsDirectory);

                string externaltexturl = config["AppSettings:externaltexturl"];
                if (string.IsNullOrEmpty(externaltexturl))
                {
                    Console.ForegroundColor = ConsoleColor.Red;
                    Console.WriteLine("Error: External Text URL is not configured.");
                    Console.ForegroundColor = ConsoleColor.Gray;
                    return;
                }

                string questsurl = config["AppSettings:questsurl"];
                if (string.IsNullOrEmpty(questsurl))
                {
                    Console.ForegroundColor = ConsoleColor.Red;
                    Console.WriteLine("Error: Quests URL is not configured.");
                    Console.ForegroundColor = ConsoleColor.Gray;
                    return;
                }

                if (File.Exists(tempFilePath))
                {
                    File.Delete(tempFilePath);
                }

                httpClient.EnsureUserAgent();

                await DownloadFileAsync(externaltexturl, tempFilePath, "external_texts.txt");

                Console.WriteLine("External Flash Texts Downloaded...");
                Console.WriteLine("Begin parsing...");

                string[] names = [.. ParseQuestImageNames(File.ReadLines(tempFilePath))];
                Console.WriteLine($"{names.Length:N0} quest images named in the texts; downloading the missing ones...");

                int downloaded = 0, absent = 0;
                string baseUrl = questsurl.TrimEnd('/');
                await Parallel.ForEachAsync(names, new ParallelOptions { MaxDegreeOfParallelism = MaxParallelDownloads },
                    async (name, _) =>
                    {
                        string fileName = $"{name}.png";
                        string filePath = Path.Combine(QuestsDirectory, fileName);
                        if (File.Exists(filePath)) return;

                        if (await TryDownloadQuestImageAsync($"{baseUrl}/{fileName}", filePath, fileName))
                            Interlocked.Increment(ref downloaded);
                        else
                            Interlocked.Increment(ref absent);
                    });

                Console.ForegroundColor = ConsoleColor.Green;
                Console.WriteLine("Finished Downloading Quest images!");
                Console.WriteLine($"{downloaded} images have been downloaded!");
                Console.ForegroundColor = ConsoleColor.Gray;
                // Most quests never had an image of their own; the texts name them all the same.
                if (absent > 0) Console.WriteLine($"{absent:N0} of the named quests have no image on the server.");
            }
            finally
            {
                // Only this tool's own file: ./temp also holds the badge texts of another download.
                if (File.Exists(tempFilePath)) File.Delete(tempFilePath);
            }
        }

        /// <summary>
        /// The quest image names behind the "quests.*" keys of external_flash_texts, without repeats.
        /// A key reads "quests.&lt;quest&gt;.&lt;field&gt;" or "quests.&lt;campaign&gt;.&lt;quest&gt;.&lt;field&gt;", and the
        /// server has both "&lt;campaign&gt;.png" and "&lt;campaign&gt;_&lt;quest&gt;.png", so both are asked for.
        /// <para>
        /// Only the part before the "=" is read. Splitting the whole line on "." made the text itself
        /// part of the name, so "quests.crafting_hween23_5.name=il grande creatore" asked for
        /// "crafting_hween23_5_name=il grande creatore.png" and the real crafting_hween23_5.png was
        /// never downloaded.
        /// </para>
        /// </summary>
        public static IReadOnlyList<string> ParseQuestImageNames(IEnumerable<string> lines)
        {
            var names = new List<string>();
            var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

            foreach (string line in lines)
            {
                if (!line.StartsWith("quests.", StringComparison.Ordinal)) continue;

                string[] segments = line.Split('=', 2)[0].Split('.', StringSplitOptions.RemoveEmptyEntries);
                // "quests.<quest>.<field>" is the shortest key that names a quest.
                if (segments.Length < 3) continue;

                if (segments.Length >= 4) Add($"{segments[1]}_{segments[2]}");
                Add(segments[1]);
            }

            return names;

            void Add(string name)
            {
                if (name.Length == 0 || name.AsSpan().IndexOfAny(Path.GetInvalidFileNameChars()) >= 0) return;
                if (seen.Add(name)) names.Add(name);
            }
        }

        /// <summary>True when the image existed and was saved; false for a quest without one.</summary>
        private static async Task<bool> TryDownloadQuestImageAsync(string url, string filePath, string fileName)
        {
            try
            {
                using var response = await httpClient.GetAsync(url);
                if (!response.IsSuccessStatusCode) return false;

                // Written next to it first, so a failed download never leaves half a .png.
                string partPath = filePath + ".part";
                await using (var fileStream = new FileStream(partPath, FileMode.Create, FileAccess.Write, FileShare.None, 4096, true))
                {
                    await response.Content.CopyToAsync(fileStream);
                }
                File.Move(partPath, filePath, overwrite: true);

                Console.ForegroundColor = ConsoleColor.Green;
                Console.WriteLine($"Downloaded: {fileName}");
                Console.ForegroundColor = ConsoleColor.Gray;
                return true;
            }
            catch (Exception ex) when (ex is HttpRequestException or IOException or TaskCanceledException)
            {
                Console.ForegroundColor = ConsoleColor.Red;
                Console.WriteLine($"{fileName} could not be downloaded: {ex.Message}");
                Console.ForegroundColor = ConsoleColor.Gray;
                return false;
            }
        }

        private static async Task DownloadFileAsync(string url, string filePath, string fileName)
        {
            try
            {
                using var response = await httpClient.GetAsync(url);
                response.EnsureSuccessStatusCode();

                using (var fileStream = new FileStream(filePath, FileMode.Create, FileAccess.Write, FileShare.None))
                {
                    await response.Content.CopyToAsync(fileStream);
                }

                Console.ForegroundColor = ConsoleColor.Green;
                Console.WriteLine($"Downloaded: {fileName}");
                Console.ForegroundColor = ConsoleColor.Gray;
            }
            catch (HttpRequestException ex)
            {
                throw;
            }
        }
    }
}
