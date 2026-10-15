Packaged remote runtime

The build preparation script verifies the official Node 24.21.0 archive checksum, stages its executable and license in generated/, and binds public Deployment source and the fixed runner to the native build manifest. Generated binaries are not committed. Unsupported clients including Windows retain the existing desktop build and manual deployment flow; no remote manifest is enabled. A build without preparation keeps remote execution disabled.
