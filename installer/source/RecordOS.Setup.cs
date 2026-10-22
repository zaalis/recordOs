using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Security.Cryptography;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Markup;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Microsoft.Win32;

[assembly: AssemblyTitle("Record OS — Installation")]
[assembly: AssemblyProduct("Record OS")]
[assembly: AssemblyCompany("Record OS")]
[assembly: AssemblyVersion("0.1.4.0")]
[assembly: AssemblyFileVersion("0.1.4.0")]

internal sealed class Setup {
    Window window;
    string destination;
    string existing;
    string stage = "Welcome";
    bool busy;
    bool captureOnly;
    const string Version = "0.1.4";
    T Find<T>(string name) where T : class { return window.FindName(name) as T; }
    static Stream Resource(string name) {
        var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(name);
        if (stream == null) throw new InvalidDataException("Ressource d’installation manquante : " + name);
        return stream;
    }
    static string ReadResource(string name) { using (var s = Resource(name)) using (var r = new StreamReader(s)) return r.ReadToEnd(); }
    static string Hash(Stream stream) { using (var sha = SHA256.Create()) return BitConverter.ToString(sha.ComputeHash(stream)).Replace("-", ""); }
    static void VerifyPayload() {
        using (var s = Resource("payload.exe")) {
            if (!String.Equals(Hash(s), ReadResource("payload.sha256").Trim(), StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("Le fichier d’installation est endommagé. Téléchargez à nouveau l’installateur.");
        }
    }
    static string RegisteredPath(string product) {
        using (var root = RegistryKey.OpenBaseKey(RegistryHive.CurrentUser, RegistryView.Registry64))
        using (var key = root.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Uninstall\" + product)) {
            if (key == null) return null;
            var location = (key.GetValue("InstallLocation") as string ?? "").Trim('"');
            if (!Path.IsPathRooted(location) || !File.Exists(Path.Combine(location, "vision-pro.exe"))) return null;
            return Path.GetFullPath(location);
        }
    }
    void Show(string name) {
        stage = name;
        foreach (string view in new [] { "Welcome", "Options", "Installing", "Finished", "Error" })
            Find<FrameworkElement>(view).Visibility = view == name ? Visibility.Visible : Visibility.Collapsed;
        Find<Button>("Secondary").Visibility = busy ? Visibility.Hidden : Visibility.Visible;
        Find<Button>("Primary").IsEnabled = !busy;
        Find<Button>("Primary").Content = name == "Finished" ? "Ouvrir Record OS" : name == "Error" ? "Réessayer" : existing != null ? "Mettre à jour" : "Installer Record OS";
        Find<Button>("Secondary").Content = name == "Welcome" ? "Personnaliser" : name == "Options" ? "Retour" : "Fermer";
        for (int i = 1; i <= 3; i++) {
            var step = Find<TextBlock>("Step" + i);
            bool active = (i == 1 && (name == "Welcome" || name == "Options" || name == "Error")) || (i == 2 && name == "Installing") || (i == 3 && name == "Finished");
            step.Foreground = new SolidColorBrush((Color)ColorConverter.ConvertFromString(active ? "#1D1D1F" : "#77777E"));
            step.FontWeight = active ? FontWeights.SemiBold : FontWeights.Normal;
        }
    }
    async Task Install() {
        if (captureOnly) return;
        try {
            if (!Environment.Is64BitOperatingSystem) throw new InvalidOperationException("Record OS nécessite Windows 10 ou 11 en 64 bits.");
            // Do not terminate an active drawing session. The user explicitly closes the app first.
            foreach (var process in Process.GetProcessesByName("vision-pro")) {
                using (process) {
                    if (!process.HasExited && process.SessionId == Process.GetCurrentProcess().SessionId)
                        throw new InvalidOperationException("Fermez l’application PC, puis cliquez sur Réessayer. Vos réglages et appareils associés seront conservés.");
                }
            }
            destination = Path.GetFullPath(destination);
            if (destination == Path.GetPathRoot(destination) || destination.Contains("\"")) throw new InvalidOperationException("Choisissez un dossier d’application valide.");
            busy = true;
            Show("Installing");
            string temp = Path.Combine(Path.GetTempPath(), "RecordOS-Setup-" + Guid.NewGuid().ToString("N"));
            await Task.Run(() => {
                Directory.CreateDirectory(temp);
                try {
                    VerifyPayload();
                    string payload = Path.Combine(temp, "install.exe");
                    using (var s = Resource("payload.exe")) using (var f = File.Create(payload)) s.CopyTo(f);
                    using (var f = File.OpenRead(payload)) {
                        if (Hash(f) != ReadResource("payload.sha256").Trim()) throw new InvalidDataException("Échec de la vérification de l’installateur.");
                    }
                    // NSIS requires /D to be last and the path to remain unquoted.
                    var info = new ProcessStartInfo(payload, "/S /D=" + destination) { UseShellExecute = false, CreateNoWindow = true };
                    using (var child = Process.Start(info)) {
                        child.WaitForExit();
                        if (child.ExitCode != 0) throw new InvalidOperationException("L’installation n’a pas abouti (code " + child.ExitCode + "). Vérifiez l’espace disponible et les droits sur le dossier choisi, puis réessayez.");
                    }
                    var exe = Path.Combine(destination, "vision-pro.exe");
                    if (!File.Exists(exe) || FileVersionInfo.GetVersionInfo(exe).ProductVersion != Version)
                        throw new InvalidDataException("La version installée n’a pas pu être vérifiée. Relancez l’installateur.");
                    if (!String.Equals(RegisteredPath("Record OS"), destination, StringComparison.OrdinalIgnoreCase))
                        throw new InvalidDataException("L’enregistrement Windows de l’application n’a pas pu être vérifié.");
                } finally {
                    try { Directory.Delete(temp, true); } catch (IOException) {} catch (UnauthorizedAccessException) {}
                }
            });
            busy = false;
            Show("Finished");
        } catch (Exception e) {
            busy = false;
            Find<TextBlock>("ErrorText").Text = e.Message;
            Show("Error");
        }
    }
    void Capture(string path) {
        window.UpdateLayout();
        var visual = (FrameworkElement)window.Content;
        // Render the actual native WPF control tree at 150%, without simulated HTML.
        var bitmap = new RenderTargetBitmap((int)Math.Ceiling(visual.ActualWidth * 1.5), (int)Math.Ceiling(visual.ActualHeight * 1.5), 144, 144, PixelFormats.Pbgra32);
        bitmap.Render(visual);
        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(bitmap));
        using (var file = File.Create(path)) encoder.Save(file);
    }
    void Run(string captureDirectory) {
        captureOnly = captureDirectory != null;
        existing = RegisteredPath("Record OS") ?? RegisteredPath("Vision-Pro");
        destination = existing ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Record OS");
        using (var reader = new System.Xml.XmlTextReader(new StringReader(ReadResource("layout.xaml")))) window = (Window)XamlReader.Load(reader);
        using (var s = Resource("logo.png")) {
            var image = new BitmapImage(); image.BeginInit(); image.CacheOption = BitmapCacheOption.OnLoad; image.StreamSource = s; image.EndInit(); image.Freeze();
            Find<Image>("BrandLogo").Source = image; window.Icon = image;
        }
        Find<TextBlock>("VersionLabel").Text = "Version " + Version;
        Find<TextBlock>("Location").Text = destination;
        if (existing != null) {
            Find<TextBlock>("UpgradeNote").Text = "Mise à jour disponible. Vos réglages et appareils associés sont conservés.";
            Find<Button>("ChooseFolder").IsEnabled = false;
            Find<Button>("ChooseFolder").Content = "Emplacement actuel conservé";
        }
        Find<Button>("Primary").Click += async (sender, args) => {
            if (stage == "Finished") {
                try { Process.Start(new ProcessStartInfo(Path.Combine(destination, "vision-pro.exe")) { UseShellExecute = true }); window.Close(); }
                catch (Exception e) { Find<TextBlock>("ErrorText").Text = "Record OS est installé, mais son ouverture a échoué. " + e.Message; Show("Error"); }
            } else await Install();
        };
        Find<Button>("Secondary").Click += (sender, args) => { if (stage == "Welcome") Show("Options"); else if (stage == "Options") Show("Welcome"); else window.Close(); };
        Find<Button>("ChooseFolder").Click += (sender, args) => {
            using (var dialog = new System.Windows.Forms.FolderBrowserDialog()) {
                dialog.Description = "Choisissez le dossier de Record OS";
                dialog.SelectedPath = destination;
                if (dialog.ShowDialog() == System.Windows.Forms.DialogResult.OK) {
                    destination = dialog.SelectedPath;
                    Find<TextBlock>("Location").Text = destination;
                }
            }
        };
        window.Closing += (sender, args) => { if (busy) args.Cancel = true; };
        Show("Welcome");
        if (captureOnly) {
            window.Loaded += (sender, args) => {
                var timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
                int step = 0;
                timer.Tick += (s, e) => {
                    Directory.CreateDirectory(captureDirectory);
                    if (step == 0) { Capture(Path.Combine(captureDirectory, "01-bienvenue.png")); Show("Options"); }
                    else if (step == 1) Capture(Path.Combine(captureDirectory, "02-personnalisation.png"));
                    else { timer.Stop(); window.Close(); }
                    step++;
                };
                timer.Start();
            };
        }
        new Application().Run(window);
    }
    [STAThread] static int Main(string[] args) {
        try {
            if (args.Length == 2 && args[0] == "--verify-payload") {
                VerifyPayload();
                File.WriteAllText(args[1], "Payload SHA-256: OK\r\nVersion: " + Version + "\r\n");
                return 0;
            }
            bool created;
            using (var mutex = new System.Threading.Mutex(true, @"Local\RecordOS.Setup", out created)) {
                if (!created) { MessageBox.Show("L’installateur Record OS est déjà ouvert.", "Record OS", MessageBoxButton.OK, MessageBoxImage.Information); return 1; }
                try { new Setup().Run(args.Length == 2 && args[0] == "--capture-dir" ? Path.GetFullPath(args[1]) : null); }
                finally { mutex.ReleaseMutex(); }
            }
            return 0;
        } catch (Exception e) {
            MessageBox.Show(e.Message, "Record OS — Installation", MessageBoxButton.OK, MessageBoxImage.Error);
            return 1;
        }
    }
}
