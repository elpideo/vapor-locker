//! Modèles de données pour les opérations clé-valeur.
//!
//! Définit les structures d'entrée et les erreurs de validation.

/// Paramètres pour une opération SET (stockage clé-valeur).
#[derive(Debug, Clone)]
pub struct SetInput {
    pub key: String,
    pub value: String,
    pub ephemeral: bool,
}

/// Paramètres pour une opération GET (récupération par clé).
#[derive(Debug, Clone)]
pub struct GetInput {
    pub key: String,
}

/// Erreur de validation des entrées (clé ou valeur trop longue).
#[derive(Debug)]
pub enum ValidationError {
    KeyTooLong { max: usize, got: usize },
    ValueTooLong { max: usize, got: usize },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::KeyTooLong { max, got } => {
                write!(f, "key too long (max {max} chars, got {got})")
            }
            ValidationError::ValueTooLong { max, got } => {
                write!(f, "value too long (max {max} chars, got {got})")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

impl SetInput {
    /// Valide la clé (max 255 caractères) et la valeur (max 100 000 caractères).
    pub fn validate(self) -> Result<Self, ValidationError> {
        let key_len = self.key.chars().count();
        if key_len > 255 {
            return Err(ValidationError::KeyTooLong {
                max: 255,
                got: key_len,
            });
        }
        let value_len = self.value.chars().count();
        if value_len > 500_000 {
            return Err(ValidationError::ValueTooLong {
                max: 500_000,
                got: value_len,
            });
        }
        Ok(self)
    }
}

impl GetInput {
    /// Valide la clé (max 255 caractères).
    pub fn validate(self) -> Result<Self, ValidationError> {
        let key_len = self.key.chars().count();
        if key_len > 255 {
            return Err(ValidationError::KeyTooLong {
                max: 255,
                got: key_len,
            });
        }
        Ok(self)
    }
}

