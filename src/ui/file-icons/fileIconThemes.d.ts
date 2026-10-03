/** Types the build-time association tables and assets shared by the file-tree themes. */
declare module "virtual:file-icon-themes" {
  export const fileIconThemes: Record<"material" | "catppuccin", {
    fileNames: Record<string, string>;
    fileExtensions: Record<string, string>;
    folderNames: Record<string, string>;
    folderNamesExpanded: Record<string, string>;
    file: string;
    folder: string;
    folderExpanded: string;
    iconUrls: Record<string, string>;
    licenseUrl: string;
  }>;
}
