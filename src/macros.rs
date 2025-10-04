//! Macros for reducing boilerplate in output module registration and configuration
//!
//! This module provides declarative macros that automatically generate:
//! - CLI argument definitions for output modules
//! - Module registration and instantiation logic
//! - OutputModuleBuilder implementations
//!
//! This approach reduces ~135 lines of repetitive code to ~15 lines of declarations.

/// Macro to generate CLI arguments for output modules
///
/// Usage:
/// ```
/// output_modules_args! {
///     beast: {
///         default_enabled: true,
///         default_port: 30005,
///         description: "Enable BEAST mode output (dump1090 compatible)"
///     },
///     raw: {
///         default_enabled: true,
///         default_port: 30002,
///         description: "Enable raw format output (dump1090 port 30002 compatible)"
///     }
/// }
/// ```
#[macro_export]
macro_rules! output_modules_args {
    ($($module:ident: {
        default_enabled: $default:expr,
        default_port: $port:expr,
        description: $desc:expr
    }),* $(,)?) => {
        $(
            // Enable flag
            #[doc = $desc]
            #[arg(long, default_value_t = $default)]
            pub $module: bool,

            // Disable flag (only for modules enabled by default)
            paste::paste! {
                #[doc = concat!("Disable ", stringify!($module), " format output")]
                #[arg(long, conflicts_with = stringify!($module))]
                pub [<no_ $module>]: bool,
            }

            // Port configuration
            paste::paste! {
                #[doc = concat!("Port for ", stringify!($module), " format output")]
                #[arg(long, default_value_t = $port)]
                pub [<$module _port>]: u16,
            }
        )*
    };
}

/// Macro to generate module registration logic
///
/// Usage:
/// ```
/// register_output_modules! {
///     manager: output_manager,
///     args: args,
///     modules: [
///         beast: {
///             type: BeastOutput,
///             enabled_check: args.beast && !args.no_beast,
///             port_field: args.beast_port,
///             name: "beast",
///             success_msg: "BEAST mode server started on port {}",
///             error_msg: "Failed to start BEAST server"
///         },
///         // ... other modules
///     ]
/// }
/// ```
#[macro_export]
macro_rules! register_raw_module {
    ($manager:ident, $module:expr) => {
        $manager.add_raw_module(Box::new($module))
    };
}

#[macro_export]
macro_rules! register_state_module {
    ($manager:ident, $module:expr) => {
        $manager.add_state_module(Box::new($module))
    };
}

#[macro_export]
macro_rules! register_output_modules {
    (
        manager: $manager:ident,
        args: $args:ident,
        modules: [
            $($module:ident: {
                type: $module_type:ty,
                enabled_check: $enabled:expr,
                port_field: $port:expr,
                name: $name:expr,
                success_msg: $success:expr,
                error_msg: $error:expr,
                kind: raw
            }),* $(,)?
            $($state_module:ident: {
                type: $state_module_type:ty,
                enabled_check: $state_enabled:expr,
                port_field: $state_port:expr,
                name: $state_name:expr,
                success_msg: $state_success:expr,
                error_msg: $state_error:expr,
                kind: state
            }),* $(,)?
        ]
    ) => {
        // Register raw modules
        $(
            if $enabled {
                let config = $crate::OutputModuleConfig::new($name, $port)
                    .with_buffer_capacity(1024);
                match <$module_type>::new(config).await {
                    Ok(module) => {
                        println!($success, $port);
                        $manager.add_raw_module(Box::new(module));
                    }
                    Err(e) => {
                        eprintln!("{}: {}", $error, e);
                    }
                }
            }
        )*

        // Register state modules
        $(
            if $state_enabled {
                let config = $crate::OutputModuleConfig::new($state_name, $state_port)
                    .with_buffer_capacity(1024);
                match <$state_module_type>::new(config).await {
                    Ok(module) => {
                        println!($state_success, $state_port);
                        $manager.add_state_module(Box::new(module));
                    }
                    Err(e) => {
                        eprintln!("{}: {}", $state_error, e);
                    }
                }
            }
        )*
    };
}

/// Macro to implement OutputModuleBuilder for a module type
///
/// Usage:
/// ```
/// impl_output_builder! {
///     BeastOutput => BeastOutputBuilder {
///         module_type: "beast",
///         description: "BEAST binary protocol for dump1090 compatibility",
///         default_port: 30005
///     }
/// }
/// ```
#[macro_export]
macro_rules! impl_output_builder {
    (
        $output_type:ty => $builder_type:ident {
            module_type: $type_name:expr,
            description: $desc:expr,
            default_port: $port:expr
        }
    ) => {
        /// Builder for output modules
        pub struct $builder_type;

        impl $builder_type {
            pub fn new() -> Self {
                Self
            }
        }

        #[async_trait::async_trait]
        impl $crate::OutputModuleBuilder for $builder_type {
            fn module_type(&self) -> &str {
                $type_name
            }

            fn description(&self) -> &str {
                $desc
            }

            fn default_port(&self) -> u16 {
                $port
            }

            async fn build(
                &self,
                config: $crate::OutputModuleConfig
            ) -> anyhow::Result<Box<dyn $crate::OutputModule>> {
                let module = <$output_type>::new(config).await?;
                Ok(Box::new(module))
            }
        }
    };
}

/// Macro to define the complete set of output modules with all their metadata
/// This serves as the single source of truth for all module configurations
#[macro_export]
macro_rules! define_output_modules {
    ($($module:ident: {
        type: $module_type:ty,
        builder: $builder_type:ident,
        default_enabled: $default:expr,
        default_port: $port:expr,
        description: $desc:expr,
        success_msg: $success:expr,
        error_msg: $error:expr
    }),* $(,)?) => {
        // Generate CLI arguments
        paste::paste! {
            $(
                /// $desc
                #[arg(long, default_value_t = $default)]
                pub $module: bool,

                #[doc = concat!("Disable ", stringify!($module), " format output")]
                #[arg(long, conflicts_with = stringify!($module))]
                pub [<no_ $module>]: bool,

                #[doc = concat!("Port for ", stringify!($module), " format output")]
                #[arg(long, default_value_t = $port)]
                pub [<$module _port>]: u16,
            )*
        }

        // Generate module registration function
        pub async fn register_all_modules(
            manager: &mut $crate::output_module::OutputModuleManager,
            args: &Args
        ) {
            paste::paste! {
                $(
                    if args.$module && !args.[<no_ $module>] {
                        let config = $crate::output_module::OutputModuleConfig::new(
                            stringify!($module),
                            args.[<$module _port>]
                        ).with_buffer_capacity(1024);

                        match <$module_type>::new(config).await {
                            Ok(module) => {
                                println!($success, args.[<$module _port>]);
                                manager.add_module(Box::new(module));
                            }
                            Err(e) => {
                                eprintln!("{}: {}", $error, e);
                            }
                        }
                    }
                )*
            }
        }

        // Generate builder registration function
        pub fn register_all_builders(registry: &mut $crate::output_module::OutputModuleRegistry) {
            $(
                registry.register($builder_type::new());
            )*
        }
    };
}